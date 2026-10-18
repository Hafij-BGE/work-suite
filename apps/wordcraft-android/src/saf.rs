//! Android's own file picker and "Open with", through `WriterActivity` (Java, in
//! `packaging/android`): the editor gets plain file paths, and Java copies them from and back to
//! the documents the user picked (see the activity's comment).
//!
//! Every call is a quick JNI call on the app's thread; the picker answers later, polled each frame.

use jni::objects::{GlobalRef, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};
use winit::platform::android::activity::AndroidApp;

pub struct Saf {
    vm: JavaVM,
    activity: GlobalRef,
}

impl Saf {
    /// The bridge to this app's activity, or `None` (logged) when JNI isn't reachable.
    pub fn new(app: &AndroidApp) -> Option<Saf> {
        // SAFETY: android-activity hands out the process's JavaVM and the activity object, valid
        // for as long as the app runs; the activity is held through a global reference.
        #[allow(unsafe_code)]
        let (vm, activity) = unsafe {
            let vm = JavaVM::from_raw(app.vm_as_ptr().cast());
            let activity = JObject::from_raw(app.activity_as_ptr().cast());
            (vm, activity)
        };
        let vm = vm.map_err(|e| log::warn!("no JavaVM: {e}")).ok()?;
        let activity = {
            let env = vm.attach_current_thread_permanently().map_err(|e| log::warn!("JNI attach: {e}")).ok()?;
            env.new_global_ref(activity).map_err(|e| log::warn!("activity reference: {e}")).ok()?
        };
        Some(Saf { vm, activity })
    }

    /// Show the system picker to open a file (`purpose`: `document`, `picture`…).
    pub fn pick_open(&self, purpose: &str) -> bool {
        self.call_with_string("pickOpen", purpose)
    }

    /// Show the system picker to choose where to save `name`.
    pub fn pick_save(&self, name: &str) -> bool {
        self.call_with_string("pickSave", name)
    }

    /// The picker's answer once it has one: `Some("")` when cancelled, else the path to use.
    pub fn take_result(&self) -> Option<String> {
        self.call_for_string("takeResult")
    }

    /// The next document another app opened with this one.
    pub fn take_opened(&self) -> Option<String> {
        self.call_for_string("takeOpened")
    }

    /// Copy documents saved since the last call back to where they came from (in the background).
    pub fn sync_back(&self) {
        self.call("syncBack", |env, activity| env.call_method(activity, "syncBack", "()V", &[]).map(|_| None));
    }

    fn call_with_string(&self, method: &str, arg: &str) -> bool {
        self.call(method, |env, activity| {
            let s = env.new_string(arg)?;
            env.call_method(activity, method, "(Ljava/lang/String;)V", &[JValue::Object(&s)])?;
            Ok(Some(String::new()))
        })
        .is_some()
    }

    fn call_for_string(&self, method: &str) -> Option<String> {
        self.call(method, |env, activity| {
            let value = env.call_method(activity, method, "()Ljava/lang/String;", &[])?.l()?;
            if value.is_null() {
                return Ok(None);
            }
            Ok(Some(env.get_string(&JString::from(value))?.into()))
        })
    }

    /// Run `f` inside a JNI local frame: this thread stays attached and never returns to Java, so
    /// local references would otherwise pile up until the VM aborts. Errors are logged and the
    /// pending Java exception cleared.
    fn call(&self, what: &str, f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<Option<String>>) -> Option<String> {
        let mut env = self.vm.attach_current_thread_permanently().map_err(|e| log::warn!("JNI attach: {e}")).ok()?;
        let activity = self.activity.as_obj();
        match env.with_local_frame(8, |env| f(env, activity)) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("{what}: {e}");
                if env.exception_check().unwrap_or(false) {
                    let _ = env.exception_clear();
                }
                None
            }
        }
    }
}
