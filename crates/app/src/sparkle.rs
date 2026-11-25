#[cfg(target_os = "macos")]
mod here {
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{MainThreadMarker, msg_send};
    use objc2_foundation::{NSBundle, NSString};
    use std::cell::OnceCell;

    thread_local! {
        static CONTROLLER: OnceCell<Option<Retained<AnyObject>>> = const { OnceCell::new() };
    }

    fn controller() -> Option<Retained<AnyObject>> {
        MainThreadMarker::new()?;
        let frameworks = NSBundle::mainBundle().privateFrameworksPath()?;
        let path = NSString::from_str(&format!("{frameworks}/Sparkle.framework"));
        let bundle = NSBundle::bundleWithPath(&path)?;
        if !unsafe { bundle.load() } {
            return None;
        }
        let class = AnyClass::get(c"SPUStandardUpdaterController")?;
        let made: Allocated<AnyObject> = unsafe { msg_send![class, alloc] };
        unsafe {
            msg_send![
                made,
                initWithStartingUpdater: true,
                updaterDelegate: None::<&AnyObject>,
                userDriverDelegate: None::<&AnyObject>
            ]
        }
    }

    fn with<R>(f: impl FnOnce(&AnyObject) -> R) -> Option<R> {
        CONTROLLER.with(|c| c.get_or_init(controller).as_deref().map(f))
    }

    fn set_daily(controller: &AnyObject, on: bool) {
        let updater: Option<Retained<AnyObject>> = unsafe { msg_send![controller, updater] };
        if let Some(updater) = updater {
            let _: () = unsafe { msg_send![&*updater, setAutomaticallyChecksForUpdates: on] };
        }
    }

    pub fn start(daily: bool) -> bool {
        with(|c| set_daily(c, daily)).is_some()
    }

    pub fn running() -> bool {
        CONTROLLER.with(|c| c.get().is_some_and(Option::is_some))
    }

    pub fn daily(on: bool) {
        if running() {
            with(|c| set_daily(c, on));
        }
    }

    pub fn check_now() -> bool {
        running()
            && with(|c| {
                let _: () = unsafe { msg_send![c, checkForUpdates: None::<&AnyObject>] };
            })
            .is_some()
    }
}

#[cfg(not(target_os = "macos"))]
mod here {
    pub fn start(_daily: bool) -> bool {
        false
    }

    pub fn running() -> bool {
        false
    }

    pub fn daily(_on: bool) {}

    pub fn check_now() -> bool {
        false
    }
}

pub use here::{check_now, daily, running, start};
