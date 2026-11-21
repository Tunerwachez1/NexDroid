//! Read-only version information for the About page.
#[derive(Default)]
pub struct AboutInfo {
    pub waydroid: Option<String>,
    pub android: Option<String>,
    pub sdk: Option<String>,
    pub abis: Option<String>,
}

pub async fn read() -> AboutInfo {
    use std::time::Duration;
    let mut info = AboutInfo::default();
    let mut command = tokio::process::Command::new("waydroid");
    command.arg("--version").kill_on_drop(true);
    if let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(4),command.output()).await {
        if output.status.success() {
            info.waydroid = String::from_utf8_lossy(&output.stdout).lines()
                .find(|line|!line.trim().is_empty()).map(|line|line.trim().chars().take(160).collect());
        }
    }
    let android = tokio::time::timeout(Duration::from_secs(8),async {
        let helper = liw_core::helper::HelperClient::connect().await?;
        let (version,sdk,abis) = tokio::try_join!(
            helper.get_prop("ro.build.version.release"),
            helper.get_prop("ro.build.version.sdk"),
            helper.get_prop("ro.product.cpu.abilist"),
        )?;
        Ok::<_,liw_core::helper::HelperError>((version,sdk,abis))
    }).await;
    if let Ok(Ok((version,sdk,abis))) = android {
        let text = |value:String| (!value.trim().is_empty()).then(||value.trim().to_owned());
        info.android=text(version); info.sdk=text(sdk); info.abis=text(abis);
    }
    info
}
