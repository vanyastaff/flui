//! Which `FluiActivity` instance runs FLUI: a process runs one platform, in a
//! task of its own application (ADR-0366 §2).
//!
//! `FluiHost.admit` decides, in `FluiActivity.onCreate` before
//! `NativeActivity` starts the library, so the decision is Java's: nothing of
//! this library runs yet. This is that decision as data, the table of record,
//! with the life of the owner it rests on: an instance becomes the owner, or
//! forwards what it was opened with to the application's own task
//! ([`forward_flags`]), or, already forwarded and still beside a live owner,
//! hands it over through the queue. `fluihost_applies_this_model` holds the
//! Java side to it statement by statement.

/// What a new instance does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admission {
    /// It owns `FluiHost`'s queues and runs FLUI (`FluiHost.OWN`).
    Own,
    /// It starts the Activity again in its application's own task with what
    /// it was opened with, and finishes (`FluiHost.FORWARD`).
    Forward,
    /// It was forwarded and still finds a live owner: it queues what it was
    /// opened with for the owner and finishes (`FluiHost.HAND_OVER`).
    HandOver,
}

/// One Activity instance, as the model tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Instance(pub(crate) u32);

/// The owner of `FluiHost`'s queues through the lifecycle of the instances.
#[derive(Debug, Default)]
pub(crate) struct Ownership {
    /// The owner, and whether it is finishing.
    owner: Option<(Instance, bool)>,
}

impl Ownership {
    /// `FluiHost.admit`: what `instance` does, created at the root of its
    /// task or not, by a start [`forward_flags`] made or not.
    pub(crate) fn created(
        &mut self,
        instance: Instance,
        task_root: bool,
        forwarded: bool,
    ) -> Admission {
        if self
            .owner
            .is_some_and(|(owner, finishing)| owner != instance && !finishing)
        {
            return if forwarded {
                Admission::HandOver
            } else {
                Admission::Forward
            };
        }
        if !task_root && !forwarded {
            return Admission::Forward;
        }
        self.owner = Some((instance, false));
        Admission::Own
    }

    /// `Activity.finish`: `isFinishing` answers `true` from here on.
    pub(crate) fn finishing(&mut self, instance: Instance) {
        if let Some((owner, finishing)) = &mut self.owner
            && *owner == instance
        {
            *finishing = true;
        }
    }

    /// `FluiHost.stopped`: a stopped owner keeps the queues; it may start
    /// again.
    #[expect(
        clippy::unused_self,
        reason = "the model names every event of the owner's life, and a stop changes nothing"
    )]
    pub(crate) fn stopped(&mut self, instance: Instance) {
        let _ = instance;
    }

    /// `FluiHost.destroyed`: a destroyed owner gives the queues up.
    pub(crate) fn destroyed(&mut self, instance: Instance) {
        if self.owner.is_some_and(|(owner, _)| owner == instance) {
            self.owner = None;
        }
    }

    /// `FluiHost.owns`.
    pub(crate) fn owns(&self, instance: Instance) -> bool {
        self.owner.is_some_and(|(owner, _)| owner == instance)
    }
}

/// `Intent.FLAG_GRANT_READ_URI_PERMISSION`.
pub(crate) const FLAG_GRANT_READ_URI_PERMISSION: i32 = 0x0000_0001;
/// `Intent.FLAG_GRANT_WRITE_URI_PERMISSION`.
pub(crate) const FLAG_GRANT_WRITE_URI_PERMISSION: i32 = 0x0000_0002;
/// `Intent.FLAG_ACTIVITY_CLEAR_TASK`.
pub(crate) const FLAG_ACTIVITY_CLEAR_TASK: i32 = 0x0000_8000;
/// `Intent.FLAG_ACTIVITY_NEW_DOCUMENT`.
pub(crate) const FLAG_ACTIVITY_NEW_DOCUMENT: i32 = 0x0008_0000;
/// `Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY`.
pub(crate) const FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY: i32 = 0x0010_0000;
/// `Intent.FLAG_ACTIVITY_MULTIPLE_TASK`.
pub(crate) const FLAG_ACTIVITY_MULTIPLE_TASK: i32 = 0x0800_0000;
/// `Intent.FLAG_ACTIVITY_NEW_TASK`.
pub(crate) const FLAG_ACTIVITY_NEW_TASK: i32 = 0x1000_0000;

/// `FluiHost.FORWARD_ADDED_FLAGS`.
const FORWARD_ADDED_FLAGS: i32 = FLAG_ACTIVITY_NEW_TASK | FLAG_GRANT_READ_URI_PERMISSION;
/// `FluiHost.FORWARD_DROPPED_FLAGS`.
const FORWARD_DROPPED_FLAGS: i32 = FLAG_ACTIVITY_MULTIPLE_TASK
    | FLAG_ACTIVITY_NEW_DOCUMENT
    | FLAG_ACTIVITY_CLEAR_TASK
    | FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY;

/// `FluiHost.forwardFlags`: the flags of the start an instance forwards,
/// from the flags of the intent it received. The start lands in the
/// application's own task (`NEW_TASK`, no further task or document, no
/// cleared task) and grants the running instance what the received intent
/// granted this one, read access at least: its data and its clip data, into
/// which Android moves a share's streams.
pub(crate) const fn forward_flags(received: i32) -> i32 {
    (received & !FORWARD_DROPPED_FLAGS) | FORWARD_ADDED_FLAGS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::owner::android::java_host::{ACTIVITY_CLASS, HOST_CLASS, java_source};

    const A: Instance = Instance(1);
    const B: Instance = Instance(2);

    /// What happened to the first instance before the second is created.
    #[derive(Debug, Clone, Copy)]
    enum Life {
        Started,
        Stopped,
        Finishing,
        Destroyed,
    }

    /// The owner `A` through `life`, then what a second instance `B` does,
    /// created at the root of its task or not, forwarded or not.
    fn second(life: Option<Life>, task_root: bool, forwarded: bool) -> (Admission, Ownership) {
        let mut ownership = Ownership::default();
        if let Some(life) = life {
            assert_eq!(ownership.created(A, true, false), Admission::Own);
            match life {
                Life::Started => {}
                Life::Stopped => ownership.stopped(A),
                Life::Finishing => ownership.finishing(A),
                Life::Destroyed => {
                    ownership.finishing(A);
                    ownership.stopped(A);
                    ownership.destroyed(A);
                }
            }
        }
        (ownership.created(B, task_root, forwarded), ownership)
    }

    /// A second instance created in each state of the first: beside a live
    /// owner, started or stopped, it forwards, and once forwarded hands over;
    /// a finishing or destroyed owner gives way to it at the root of a task;
    /// an instance in another application's task (not its root) forwards
    /// whatever the owner, so the owner always lives in a task the launcher
    /// brings forward.
    #[test]
    fn one_instance_owns_the_queues_and_lives_in_its_applications_task() {
        use Admission::{Forward, HandOver, Own};
        for (case, life, task_root, forwarded, admission) in [
            ("the first, at its task's root", None, true, false, Own),
            (
                "the first, in another app's task",
                None,
                false,
                false,
                Forward,
            ),
            ("the first, forwarded there", None, true, true, Own),
            ("the first, forwarded onto a picker", None, false, true, Own),
            (
                "beside a started owner",
                Some(Life::Started),
                true,
                false,
                Forward,
            ),
            (
                "beside a stopped owner",
                Some(Life::Stopped),
                true,
                false,
                Forward,
            ),
            (
                "a share beside a stopped owner",
                Some(Life::Stopped),
                false,
                false,
                Forward,
            ),
            (
                "forwarded, beside a live owner",
                Some(Life::Started),
                true,
                true,
                HandOver,
            ),
            (
                "forwarded, beside a stopped owner",
                Some(Life::Stopped),
                false,
                true,
                HandOver,
            ),
            (
                "a relaunch while the owner finishes",
                Some(Life::Finishing),
                true,
                false,
                Own,
            ),
            (
                "a share while the owner finishes",
                Some(Life::Finishing),
                false,
                false,
                Forward,
            ),
            (
                "after the owner is destroyed",
                Some(Life::Destroyed),
                true,
                false,
                Own,
            ),
        ] {
            let (admitted, ownership) = second(life, task_root, forwarded);
            assert_eq!(admitted, admission, "{case}");
            assert_eq!(ownership.owns(B), admission == Own, "{case}: B owns");
            let a_owns =
                life.is_some() && admission != Own && !matches!(life, Some(Life::Destroyed));
            assert_eq!(ownership.owns(A), a_owns, "{case}: A owns");
        }

        // The owner itself, created again (a configuration change Android
        // recreates the Activity for destroys it first), owns again.
        let mut ownership = Ownership::default();
        assert_eq!(ownership.created(A, true, false), Admission::Own);
        ownership.destroyed(A);
        assert_eq!(ownership.created(B, true, false), Admission::Own);
        ownership.destroyed(A);
        assert!(
            ownership.owns(B),
            "a late destruction of the old one changes nothing"
        );
    }

    /// The start an instance forwards lands in the application's own task and
    /// carries the read grant, keeps every grant it received, and drops a
    /// further task, a new document, a cleared task and a launch from history.
    #[test]
    fn a_forwarded_start_keeps_the_grant_and_lands_in_the_applications_task() {
        for (case, received, forwarded) in [
            (
                "a plain launch",
                0,
                FLAG_ACTIVITY_NEW_TASK | FLAG_GRANT_READ_URI_PERMISSION,
            ),
            (
                "a share that grants reading and writing",
                FLAG_GRANT_READ_URI_PERMISSION | FLAG_GRANT_WRITE_URI_PERMISSION,
                FLAG_ACTIVITY_NEW_TASK
                    | FLAG_GRANT_READ_URI_PERMISSION
                    | FLAG_GRANT_WRITE_URI_PERMISSION,
            ),
            (
                "a document launch into a task of its own",
                FLAG_ACTIVITY_NEW_DOCUMENT | FLAG_ACTIVITY_MULTIPLE_TASK | FLAG_ACTIVITY_NEW_TASK,
                FLAG_ACTIVITY_NEW_TASK | FLAG_GRANT_READ_URI_PERMISSION,
            ),
            (
                "a launch that clears its task, from history",
                FLAG_ACTIVITY_CLEAR_TASK | FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY,
                FLAG_ACTIVITY_NEW_TASK | FLAG_GRANT_READ_URI_PERMISSION,
            ),
        ] {
            assert_eq!(forward_flags(received), forwarded, "{case}");
        }
    }

    /// The body of the Java method whose signature starts with `signature`.
    fn method_body<'a>(source: &'a str, signature: &str) -> &'a str {
        let start = source
            .find(signature)
            .unwrap_or_else(|| panic!("{signature:?} in the Java source"));
        let rest = &source[start..];
        let end = rest.find("\n    }\n").expect("the method ends");
        &rest[..end]
    }

    /// `FluiHost` and `FluiActivity` apply this model: the same order of
    /// conditions in `admit`, ownership given up on destruction only, the
    /// same flags and constants, and `onCreate` deciding before it restores
    /// and before `NativeActivity` starts the library.
    #[test]
    fn fluihost_applies_this_model() {
        let host = java_source(HOST_CLASS);
        let admit = method_body(
            &host,
            "static int admit(Activity activity, boolean taskRoot, boolean forwarded)",
        );
        let live = admit
            .find("if (owner != null && owner != activity && !owner.isFinishing()) {\n                return forwarded ? HAND_OVER : FORWARD;")
            .unwrap_or_else(|| panic!("a live owner first:\n{admit}"));
        let foreign = admit
            .find("if (!taskRoot && !forwarded) {\n                return FORWARD;")
            .unwrap_or_else(|| panic!("another app's task next:\n{admit}"));
        let own = admit
            .find("owner = activity;\n            return OWN;")
            .unwrap_or_else(|| panic!("then it owns:\n{admit}"));
        assert!(live < foreign && foreign < own, "{admit}");

        let destroyed = method_body(&host, "static void destroyed(Activity activity)");
        assert!(
            destroyed.contains("if (owner == activity) {\n                owner = null;"),
            "{destroyed}"
        );
        for keeps in [
            method_body(&host, "static void stopped(Activity activity)"),
            method_body(&host, "static void started(Activity activity)"),
        ] {
            assert!(
                !keeps.contains("owner ="),
                "only destruction ends ownership:\n{keeps}"
            );
        }

        for stated in [
            "static final int OWN = 0;",
            "static final int FORWARD = 1;",
            "static final int HAND_OVER = 2;",
            "static final int FORWARD_ADDED_FLAGS =\n            Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_GRANT_READ_URI_PERMISSION;",
            "static final int FORWARD_DROPPED_FLAGS = Intent.FLAG_ACTIVITY_MULTIPLE_TASK\n            | Intent.FLAG_ACTIVITY_NEW_DOCUMENT\n            | Intent.FLAG_ACTIVITY_CLEAR_TASK\n            | Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY;",
            "return (received & ~FORWARD_DROPPED_FLAGS) | FORWARD_ADDED_FLAGS;",
            // The received intent whole (data, clip data, extras), to the
            // Activity's own component, marked with this process's token.
            "Intent intent = received == null ? new Intent(Intent.ACTION_MAIN) : new Intent(received);\n        intent.setComponent(from.getComponentName());\n        intent.setFlags(forwardFlags(received == null ? 0 : received.getFlags()));\n        intent.putExtra(FORWARDED, FORWARD_TOKEN);",
            "return intent.getLongExtra(FORWARDED, 0) == FORWARD_TOKEN;",
        ] {
            assert!(host.contains(stated), "{stated:?} in FluiHost");
        }

        let activity = java_source(ACTIVITY_CLASS);
        let on_create = method_body(
            &activity,
            "protected void onCreate(Bundle savedInstanceState)",
        );
        let order = [
            "FluiHost.admit(this, isTaskRoot(), FluiHost.isForwarded(received));",
            "if (admission == FluiHost.OWN && savedInstanceState != null) {\n            FluiHost.restore(savedInstanceState);",
            "super.onCreate(savedInstanceState);",
            "if (admission != FluiHost.OWN) {\n            leave(admission, received, savedInstanceState == null);\n            return;",
        ]
        .map(|step| {
            on_create
                .find(step)
                .unwrap_or_else(|| panic!("{step:?} in onCreate:\n{on_create}"))
        });
        assert!(order.is_sorted(), "{on_create}");
        let leave = method_body(
            &activity,
            "private void leave(int admission, Intent received, boolean fresh)",
        );
        for step in [
            "if (admission == FluiHost.FORWARD) {",
            "startActivity(FluiHost.forwarded(this, opens ? received : null));",
            "handOver(opens ? received : null);",
            "finish();",
        ] {
            assert!(leave.contains(step), "{step:?} in leave:\n{leave}");
        }
    }
}
