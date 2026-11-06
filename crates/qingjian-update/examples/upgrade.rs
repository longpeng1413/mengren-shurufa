//! 真实更新链路验收：指定旧版本、平台与缓存，检查签名索引并下载校验安装包；不执行安装。

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let current = args.next().unwrap_or_else(|| "0.3.3".into());
    let platform = args.next().unwrap_or_else(|| "windows".into());
    let cache = args
        .next()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("mengren-update-verification"));
    let target = match platform.as_str() {
        "android" => qingjian_update::Target {
            platform: "android",
            cpu: "arm64",
        },
        "windows" => qingjian_update::Target {
            platform: "windows",
            cpu: "x86_64",
        },
        _ => return Err("unsupported target".into()),
    };
    let offer = qingjian_update::check(&current, target, false)?;
    match offer {
        None => println!("{platform} {current}: up to date"),
        Some(offer) => {
            println!("{platform}: {current} -> {}", offer.version);
            let last = std::sync::atomic::AtomicU64::new(0);
            let path = qingjian_update::download_release(
                &current,
                target,
                false,
                &offer.version,
                &cache,
                "",
                |done, total| {
                    let percent = done * 100 / total.max(1);
                    // 进度回调只观察，不记录下载地址或账号。
                    let step = percent / 10;
                    if last.swap(step, std::sync::atomic::Ordering::Relaxed) != step {
                        println!("Download: {percent}%");
                    }
                    true
                },
            )?;
            println!("Verified package: {}", path.display());
        }
    }
    Ok(())
}
