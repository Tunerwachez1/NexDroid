//! Narrow Android debugging settings; no arbitrary shell or property input.
use serde::{Deserialize, Serialize};
const PATH: &str = "/var/lib/nexdroid/advanced.json";
static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub adb: bool,
    pub show_touches: bool,
    pub pointer_location: bool,
}
#[derive(Serialize)]
pub struct State {
    pub root:crate::root::State,
    pub settings: Settings,
    pub abis: String,
    pub adb_secure: bool,
    pub adb_address: String,
}
async fn prop(key: &str) -> anyhow::Result<String> {
    Ok(crate::shell_getprop(key).await?.trim().into())
}
async fn system(key: &str) -> anyhow::Result<bool> {
    let value = crate::run_dumpsys(&["settings", "get", "system", key]).await?;
    parse_switch(&value)
}
fn parse_switch(value: &str) -> anyhow::Result<bool> {
    match value.trim() {
        "1" => Ok(true),
        "0" | "null" | "" => Ok(false),
        _ => anyhow::bail!("Unexpected Android debug setting: {value}"),
    }
}
fn address(output: &str,port:u16) -> String {
    let mut words = output.split_whitespace();
    while let Some(word) = words.next() {
        if word == "inet" {
            if let Some(raw) = words.next() {
                if let Some(ip) = raw
                    .split('/')
                    .next()
                    .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok())
                {
                    return format!("{ip}:{port}");
                }
            }
        }
    }
    String::new()
}
fn listening(table:&str,port:u16)->bool {
    table.lines().any(|line| {
        let fields=line.split_whitespace().collect::<Vec<_>>();
        fields.get(3)==Some(&"0A") && fields.get(1).and_then(|s|s.rsplit_once(':'))
            .and_then(|(_,p)|u16::from_str_radix(p,16).ok())==Some(port)
    })
}
async fn tcp_listening(port:u16)->anyhow::Result<bool> {
    let table=crate::run_dumpsys(&["cat","/proc/net/tcp","/proc/net/tcp6"]).await?;
    Ok(listening(&table,port))
}
async fn read_inner() -> anyhow::Result<State> {
    anyhow::ensure!(
        prop("sys.boot_completed").await? == "1",
        "Start Android before changing debugging settings"
    );
    let port = prop("service.adb.tcp.port").await?;
    let port = if port.is_empty() {
        prop("persist.adb.tcp.port").await?
    } else {
        port
    };
    let port=port.parse::<u16>().ok().filter(|p|*p>0).unwrap_or(5555);
    let service=prop("init.svc.adbd").await?;
    let adb=service!="stopped" && !service.is_empty() && tcp_listening(port).await?;
    let settings = Settings {
        adb,
        show_touches: system("show_touches").await?,
        pointer_location: system("pointer_location").await?,
    };
    let ip = crate::run_dumpsys(&["ip", "-4", "-o", "addr", "show", "dev", "eth0"])
        .await
        .unwrap_or_default();
    Ok(State {
        root:crate::root::state().await?,
        settings,
        abis: prop("ro.product.cpu.abilist").await?,
        adb_secure: prop("ro.adb.secure").await? == "1",
        adb_address: address(&ip,port),
    })
}
pub async fn read() -> anyhow::Result<String> {
    let _lock = LOCK.lock().await;
    Ok(serde_json::to_string(&read_inner().await?)?)
}
async fn apply(desired: Settings, current: Settings) -> anyhow::Result<()> {
    if desired.adb {
        anyhow::ensure!(
            prop("ro.adb.secure").await? == "1",
            "This Android image does not require ADB authentication; network ADB cannot be enabled here"
        );
    }
    for (key, on) in [
        ("show_touches", desired.show_touches),
        ("pointer_location", desired.pointer_location),
    ] {
        crate::run_dumpsys(&["settings", "put", "system", key, if on { "1" } else { "0" }]).await?;
    }
    if desired.adb != current.adb || prop("init.svc.adbd").await?=="stopping" {
        crate::run_dumpsys(&[
            "settings",
            "put",
            "global",
            "adb_enabled",
            if desired.adb { "1" } else { "0" },
        ])
        .await?;
        crate::run_dumpsys(&[
            "setprop",
            "service.adb.tcp.port",
            if desired.adb { "5555" } else { "-1" },
        ])
        .await?;
        crate::run_dumpsys(&["setprop","ctl.stop","adbd"]).await?;
        for _ in 0..10 {
            if prop("init.svc.adbd").await?=="stopped" {break;}
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        if prop("init.svc.adbd").await?!="stopped" {
            // Some Waydroid images leave adbd in "stopping" with its socket
            // still open. Only this fixed Android process may be terminated.
            let _=crate::run_dumpsys(&["killall","-9","adbd"]).await;
        }
        for _ in 0..20 {
            if prop("init.svc.adbd").await?=="stopped" {break;}
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        anyhow::ensure!(prop("init.svc.adbd").await?=="stopped","ADB did not stop completely");
        if desired.adb {crate::run_dumpsys(&["setprop","ctl.start","adbd"]).await?;}
        for _ in 0..20 {
            let service=prop("init.svc.adbd").await?;
            let ready=if desired.adb {service=="running" && tcp_listening(5555).await?} else {service=="stopped" && !tcp_listening(5555).await?};
            if ready {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        anyhow::bail!("ADB service did not reach the requested state");
    }
    Ok(())
}
fn save(settings: &Settings) -> anyhow::Result<()> {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    let path = std::path::Path::new(PATH);
    anyhow::ensure!(
        !std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()),
        "Advanced settings path is a symlink"
    );
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(&serde_json::to_vec_pretty(settings)?)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}
pub async fn set(desired: Settings) -> anyhow::Result<String> {
    let _lock = LOCK.lock().await;
    let before = read_inner().await?;
    let change = async {
        apply(desired, before.settings).await?;
        let actual = read_inner().await?;
        anyhow::ensure!(
            actual.settings == desired,
            "Android debugging settings did not match the requested values"
        );
        save(&desired)?;
        Ok::<_, anyhow::Error>(actual)
    }
    .await;
    match change {
        Ok(state) => Ok(serde_json::to_string(&state)?),
        Err(error) => {
            let now = read_inner().await.map(|s| s.settings).unwrap_or(desired);
            if let Err(rollback) = apply(before.settings, now).await {
                anyhow::bail!("{error}; restoring previous debugging settings failed: {rollback}");
            }
            Err(error)
        }
    }
}
pub async fn restore() -> anyhow::Result<()> {
    let desired = match std::fs::read_to_string(PATH) {
        Ok(s) => serde_json::from_str::<Settings>(&s)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    set(desired).await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn only_numeric_debug_switch_values_are_accepted() {
        assert!(super::parse_switch("1\n").unwrap());
        assert!(!super::parse_switch("null").unwrap());
        assert!(super::parse_switch("something else").is_err());
    }
    #[test]
    fn connection_address_is_parsed_not_executed() {
        assert_eq!(
            super::address("2: eth0 inet 192.168.240.112/24 brd 192.168.240.255",5555),
            "192.168.240.112:5555"
        );
        assert!(super::address("inet not-an-ip/24",5555).is_empty());
    }
    #[test]
    fn tcp_readback_requires_a_listening_socket_on_the_exact_port() {
        assert!(super::listening("0: 00000000:15B3 00000000:0000 0A 0:0",5555));
        assert!(!super::listening("0: 00000000:15B3 00000000:0000 01 0:0",5555));
        assert!(!super::listening("0: 00000000:15B4 00000000:0000 0A 0:0",5555));
    }
}
