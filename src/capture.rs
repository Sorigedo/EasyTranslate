use std::process::{Child, ExitStatus};
pub fn wait(child: &mut Child, cancelled: impl Fn() -> bool) -> Result<ExitStatus, String> {
    loop {
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("截图已取消".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(40)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    }
}
