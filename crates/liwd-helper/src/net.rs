//! Network diagnosis — the Rust equivalent of `liw-net-doctor.sh`.
//!
//! Lesson learned in Phase 0: telling someone to "check your firewall" does
//! not help. The real failure appeared while our ufw rules were CORRECT; what
//! hijacked the traffic was a third tool writing its own nftables table
//! (unwall/zapret, DNAT-ing DNS to 127.0.0.53). Diagnosis must therefore
//! **name the rule**.

use serde::Serialize;
use std::{net::Ipv4Addr, process::Stdio, time::Duration};
use tokio::process::Command;

#[derive(Debug, Serialize, Default)]
pub struct NetDiagnosis {
    pub bridge_up: bool,
    pub bridge_addr: Option<String>,
    pub guest_addr: Option<Ipv4Addr>,
    pub outgoing_interface: Option<String>,
    pub android_default_route: Option<bool>,
    pub dnsmasq_running: bool,
    pub ip_forward: bool,
    pub waydroid_nat: bool,
    pub lease_present: bool,
    pub active_firewall: String,
    /// FOREIGN rules hijacking waydroid0 traffic — with their table name.
    pub hijack_rules: Vec<HijackRule>,
    pub problems: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct HijackRule {
    pub table: String,
    pub rule: String,
    pub why: String,
}

async fn out(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd).args(args).stdin(Stdio::null()).output().await
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

async fn ok(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args)
        .stdout(Stdio::null()).stderr(Stdio::null()).stdin(Stdio::null())
        .status().await.map(|s| s.success()).unwrap_or(false)
}

async fn active_firewall() -> String {
    for (svc, name) in [("ufw", "ufw"), ("firewalld", "firewalld"),
                        ("nftables", "nftables"), ("iptables", "iptables")] {
        if ok("systemctl", &["is-active", "--quiet", svc]).await {
            return name.to_string();
        }
    }
    "none".to_string()
}

/// Finds foreign rules in nftables that could hijack Waydroid traffic.
///
/// In netfilter several base chains run on the same hook and **if one says
/// drop/dnat** no accept from the others can override it. That is why traffic
/// can die even while Waydroid's own rules are correct.
fn find_hijacks(ruleset: &str) -> Vec<HijackRule> {
    let mut found = Vec::new();
    let mut table = String::new();
    for line in ruleset.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("table ") {
            table = rest.trim_end_matches(" {").to_string();
        }
        // Waydroid's own tables are legitimate; skip them.
        if table.ends_with(" lxc") || table.ends_with("liwdiag") { continue; }
        if t.contains("dport 53") && t.contains("dnat to") {
            found.push(HijackRule {
                table: table.clone(),
                rule: t.to_string(),
                why: "DNS queries are redirected to another resolver; an \
                      external packet aimed at a loopback address is refused (ECONNREFUSED)".into(),
            });
        }
    }
    found
}

fn bridge_network(cidr: &str) -> Option<(Ipv4Addr, u32, u8)> {
    let (address, prefix) = cidr.split_once('/')?;
    let address: Ipv4Addr = address.parse().ok()?;
    let prefix: u8 = prefix.parse().ok()?;
    if !(1..=30).contains(&prefix) {
        return None;
    }
    let mask = u32::MAX << (32 - prefix);
    Some((address, mask, prefix))
}

fn guest_lease(leases: &str, time: u64, cidr: &str) -> Option<Ipv4Addr> {
    let (gateway, mask, _) = bridge_network(cidr)?;
    let network = u32::from(gateway) & mask;
    leases.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let expiry = fields.next()?.parse::<u64>().ok()?;
        let address: Ipv4Addr = fields.nth(1)?.parse().ok()?;
        let ip = u32::from(address);
        ((expiry == 0 || expiry > time) && address != gateway
            && ip > network && ip < (network | !mask)).then_some(address)
    })
}

fn outgoing_interface(route: &str) -> Option<String> {
    let name = route.split_whitespace().skip_while(|word| *word != "dev").nth(1)?;
    (!matches!(name, "lo" | "waydroid0") && !name.starts_with('-') && name.len() <= 15
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)))
        .then(|| name.to_owned())
}

async fn android_default_route() -> Option<bool> {
    let command = Command::new("lxc-attach")
        .args(["-P", "/var/lib/waydroid/lxc", "-n", "waydroid", "--",
            "/system/bin/dumpsys", "connectivity"])
        .stdin(Stdio::null()).kill_on_drop(true).output();
    let result = tokio::time::timeout(Duration::from_secs(3), command).await.ok()?.ok()?;
    let output = String::from_utf8_lossy(&result.stdout);
    let network = output.lines().find_map(|line| line.trim().strip_prefix("Active default network: "))?;
    Some(network != "none")
}

pub fn ufw_rules(diagnosis: &NetDiagnosis) -> Vec<Vec<String>> {
    let rule = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect();
    let mut rules = vec![rule(&["allow", "in", "on", "waydroid0",
        "proto", "udp", "from", "any", "port", "68", "to", "any", "port", "67"])];
    if let Some((gateway, mask, prefix)) = diagnosis.bridge_addr.as_deref().and_then(bridge_network) {
        let subnet = format!("{}/{}", Ipv4Addr::from(u32::from(gateway) & mask), prefix);
        for protocol in ["udp", "tcp"] {
            rules.push(rule(&["allow", "in", "on", "waydroid0", "proto", protocol,
                "from", &subnet, "to", &gateway.to_string(), "port", "53"]));
        }
        if let Some(uplink) = &diagnosis.outgoing_interface {
            rules.push(rule(&["route", "allow", "in", "on", "waydroid0",
                "out", "on", uplink, "from", &subnet, "to", "any"]));
        }
    }
    rules
}

pub async fn diagnose() -> NetDiagnosis {
    let mut d = NetDiagnosis::default();

    let addr = out("ip", &["-4", "-o", "addr", "show", "waydroid0"]).await;
    d.bridge_up = !addr.trim().is_empty();
    d.bridge_addr = addr.split_whitespace().nth(3).map(str::to_string);

    d.dnsmasq_running = ok("pgrep", &["-f", "dnsmasq.*waydroid0"]).await;
    d.ip_forward = tokio::fs::read_to_string("/proc/sys/net/ipv4/ip_forward")
        .await.map(|s| s.trim() == "1").unwrap_or(false);
    let leases = tokio::fs::read_to_string("/var/lib/misc/dnsmasq.waydroid0.leases")
        .await.unwrap_or_default();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs();
    d.guest_addr = d.bridge_addr.as_deref().and_then(|cidr| guest_lease(&leases, now, cidr));
    d.lease_present = d.guest_addr.is_some();
    if let Some(guest) = d.guest_addr {
        // Ask the kernel about forwarded traffic. This performs a route lookup
        // only and can select a different interface from the host's route.
        let route = out("ip", &["-4", "route", "get", "1.1.1.1",
            "from", &guest.to_string(), "iif", "waydroid0"]).await;
        d.outgoing_interface = outgoing_interface(&route);
    }
    d.android_default_route = android_default_route().await;
    d.active_firewall = active_firewall().await;

    let ruleset = out("nft", &["list", "ruleset"]).await;
    d.waydroid_nat = ruleset.contains("masquerade");
    d.hijack_rules = find_hijacks(&ruleset);

    if !d.bridge_up { d.problems.push("no waydroid0 bridge".into()); }
    if !d.dnsmasq_running { d.problems.push("dnsmasq is not running".into()); }
    if !d.ip_forward { d.problems.push("ip_forward is off".into()); }
    if !d.waydroid_nat { d.problems.push("no NAT/masquerade rule".into()); }
    if !d.lease_present { d.problems.push("no current IPv4 DHCP lease on waydroid0".into()); }
    if d.lease_present && d.outgoing_interface.is_none() {
        d.problems.push("no IPv4 egress route from the Android lease".into());
    }
    if d.android_default_route == Some(false) {
        d.problems.push("Android ConnectivityService has no default network".into());
    }
    match d.active_firewall.as_str() {
        "ufw" => {
            d.recommendations.extend(ufw_rules(&d).into_iter().map(|args| format!("ufw {}", args.join(" "))));
        }
        "firewalld" => {
            d.recommendations.push("Check the zone for waydroid0 and its policy to the routed uplink zone. Allow DHCP to the host, DNS to the bridge address, and forwarding from the bridge subnet to that uplink.".into());
        }
        _ => {
            d.recommendations.push("Check active nftables or iptables input and forward chains for waydroid0. Allow UDP 68→67 for DHCP, TCP/UDP 53 to the bridge address, and established return traffic and outbound forwarding for the bridge subnet on the routed uplink.".into());
        }
    }
    for h in &d.hijack_rules {
        d.problems.push(format!("a rule in table '{}' hijacks DNS: {}", h.table, h.rule));
    }
    d
}

#[cfg(test)]
mod tests {
    use super::{find_hijacks, guest_lease, outgoing_interface, ufw_rules, NetDiagnosis};

    #[test]
    fn repair_routes_use_the_guest_source_and_the_resolved_uplink() {
        let leases = "1 aa 10.20.30.44 expired *\n20 bb 10.20.40.5 other *\n20 cc 10.20.30.60 android *";
        let bridge = "10.20.30.1/24";
        assert_eq!(guest_lease(leases, 2, bridge).unwrap().to_string(), "10.20.30.60");
        let route = "1.1.1.1 from 10.20.30.60 dev tunnel_7 \n cache iif waydroid0";
        let mut diagnosis = NetDiagnosis {
            bridge_addr: Some(bridge.into()),
            outgoing_interface: outgoing_interface(route),
            ..Default::default()
        };
        let rules = ufw_rules(&diagnosis);
        assert_eq!(rules[3].join(" "),
            "route allow in on waydroid0 out on tunnel_7 from 10.20.30.0/24 to any");
        assert!(outgoing_interface("1.1.1.1 dev lo").is_none());
        diagnosis.outgoing_interface = None;
        assert_eq!(ufw_rules(&diagnosis).len(), 3);
    }

    const REAL: &str = r#"
table ip unwall {
	chain dns_redirect {
		iifname != "enp3s0" udp dport 53 dnat to 127.0.0.53:53
	}
}
table inet lxc {
	chain input {
		iifname "waydroid0" udp dport { 53, 67 } accept
	}
}
"#;

    /// The real Phase 0 failure: unwall's DNAT rule must be found.
    #[test]
    fn finds_the_unwall_dns_hijack() {
        let h = find_hijacks(REAL);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].table, "ip unwall");
        assert!(h[0].rule.contains("dnat to 127.0.0.53"));
    }

    /// Waydroid's own accept rules must not count as hijacking.
    #[test]
    fn waydroid_own_rules_are_not_hijacks() {
        let only_lxc = "table inet lxc {\n\tiifname \"waydroid0\" udp dport { 53, 67 } accept\n}\n";
        assert!(find_hijacks(only_lxc).is_empty());
    }

    #[test]
    fn clean_ruleset_reports_nothing() {
        assert!(find_hijacks("table ip filter {\n\tchain input {\n\t}\n}\n").is_empty());
    }
}
