//! The jump list over `ICustomDestinationList`: each task a shell link to
//! the running executable with the task's arguments, quoted as the
//! executable's start-up code splits a command line
//! (`CommandLineToArgvW`), titled through `PKEY_Title`.
//!
//! The list is written to the user's profile, so each change runs on its
//! own apartment thread ([`ShellThreads`]). An empty list deletes it.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use flui_platform::capability::Resolver;
use flui_platform::{SystemFailure, Unsupported, UnsupportedReason};
use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
};
use windows::core::{HSTRING, Interface as _, PCWSTR, PWSTR};

use super::{ShellThreads, os_code, wide};
use crate::jump_list::{
    JumpListBackend, JumpListCapability, JumpListError, JumpListResult, JumpListTask,
};

/// `argument` as one argument of a command line, quoted so that
/// `CommandLineToArgvW` reads it back as given: an argument with no space,
/// tab or quote stands as it is; any other is quoted, each `"` escaped, and
/// the backslashes before a `"` (or the closing one) doubled.
pub(super) fn quote(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '\n', '\u{b}', '"']) {
        return argument.to_owned();
    }
    let mut quoted = String::with_capacity(argument.len() + 2);
    quoted.push('"');
    let mut backslashes = 0_usize;
    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push(character);
                backslashes = 0;
            }
        }
    }
    quoted.extend(std::iter::repeat_n('\\', backslashes * 2));
    quoted.push('"');
    quoted
}

/// A task as the shell link is made from it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TaskLink {
    pub(super) title: String,
    pub(super) arguments: String,
    pub(super) description: Option<String>,
}

/// The links of `tasks`.
pub(super) fn links(tasks: &[JumpListTask]) -> Vec<TaskLink> {
    tasks
        .iter()
        .map(|task| TaskLink {
            title: task.title.clone(),
            arguments: task
                .arguments
                .iter()
                .map(|argument| quote(argument))
                .collect::<Vec<String>>()
                .join(" "),
            description: task.description.clone(),
        })
        .collect::<Vec<TaskLink>>()
}

/// The jump list as the provider reaches it, on a change's thread.
pub(super) trait JumpListSystem: Send + Sync + 'static {
    /// Replace the tasks with links to `executable`; none deletes the list.
    fn commit(&self, executable: &Path, links: &[TaskLink]) -> Result<(), i32>;
}

/// [`JumpListBackend`] over `ICustomDestinationList`.
pub(crate) struct WindowsJumpList {
    system: Arc<dyn JumpListSystem>,
    threads: Rc<ShellThreads>,
}

impl WindowsJumpList {
    pub(crate) fn new(threads: Rc<ShellThreads>) -> Self {
        Self {
            system: Arc::new(DestinationLists),
            threads,
        }
    }
}

/// Write `links` through `system` for the running executable.
fn commit(system: &dyn JumpListSystem, links: &[TaskLink]) -> JumpListResult {
    let executable: PathBuf = std::env::current_exe().map_err(|error| {
        JumpListError::Failed(SystemFailure::Os(error.raw_os_error().unwrap_or(i32::MAX)))
    })?;
    system
        .commit(&executable, links)
        .map_err(|code| JumpListError::Failed(SystemFailure::Os(code)))
}

impl JumpListBackend for WindowsJumpList {
    fn set_tasks(&self, tasks: Vec<JumpListTask>, answer: Resolver<JumpListResult>) {
        let links = links(&tasks);
        let system = Arc::clone(&self.system);
        self.threads.answer(
            "flui-jump-list",
            answer,
            || {
                Err(JumpListError::Unsupported(Unsupported::of::<
                    JumpListCapability,
                >(
                    UnsupportedReason::Unavailable
                )))
            },
            move || commit(&*system, &links),
        );
    }
}

/// The real destination list.
struct DestinationLists;

fn code(error: &windows::core::Error) -> i32 {
    os_code(error.code())
}

/// A shell link to `executable` for `link`.
fn shell_link(executable: &Path, link: &TaskLink) -> windows::core::Result<IShellLinkW> {
    // SAFETY: created and used on this apartment thread only; each string
    // is a NUL-terminated buffer or `HSTRING` that outlives its call, and
    // the title's `PROPVARIANT` points at a buffer of ours that outlives
    // `SetValue`, which copies it, and is never cleared by COM.
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let path = wide(executable);
        shell_link.SetPath(PCWSTR(path.as_ptr()))?;
        shell_link.SetArguments(&HSTRING::from(link.arguments.as_str()))?;
        if let Some(description) = &link.description {
            shell_link.SetDescription(&HSTRING::from(description.as_str()))?;
        }
        let store = shell_link.cast::<IPropertyStore>()?;
        let mut title = wide(&link.title);
        let mut value = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: core::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_LPWSTR,
                    ..Default::default()
                }),
            },
        };
        (*value.Anonymous.Anonymous).Anonymous.pwszVal = PWSTR(title.as_mut_ptr());
        let key = PKEY_Title;
        store.SetValue(&raw const key, &raw const value)?;
        store.Commit()?;
        Ok(shell_link)
    }
}

impl JumpListSystem for DestinationLists {
    fn commit(&self, executable: &Path, links: &[TaskLink]) -> Result<(), i32> {
        // SAFETY: the list and collection are created and used on this
        // apartment thread only; every call is a checked method of them.
        unsafe {
            let list: ICustomDestinationList =
                CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)
                    .map_err(|error| code(&error))?;
            if links.is_empty() {
                return list
                    .DeleteList(PCWSTR::null())
                    .map_err(|error| code(&error));
            }
            let mut slots = 0_u32;
            let _removed: IObjectArray = list
                .BeginList(&raw mut slots)
                .map_err(|error| code(&error))?;
            let collection: IObjectCollection =
                CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)
                    .map_err(|error| code(&error))?;
            for link in links {
                let shell_link = shell_link(executable, link).map_err(|error| code(&error))?;
                collection
                    .AddObject(&shell_link)
                    .map_err(|error| code(&error))?;
            }
            let tasks = collection
                .cast::<IObjectArray>()
                .map_err(|error| code(&error))?;
            if let Err(error) = list.AddUserTasks(&tasks) {
                let _ = list.AbortList();
                return Err(code(&error));
            }
            list.CommitList().map_err(|error| code(&error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{TaskLink, links, quote};
    use crate::jump_list::JumpListTask;

    #[test]
    fn an_argument_is_quoted_as_the_command_line_splitter_reads_it_back() {
        let cases = [
            ("plain", "plain"),
            ("--open=a.txt", "--open=a.txt"),
            ("", "\"\""),
            ("two words", "\"two words\""),
            ("say \"hi\"", "\"say \\\"hi\\\"\""),
            // Backslashes are plain unless a quote follows them.
            (r"C:\dir\", r"C:\dir\"),
            (r"C:\a b\", r#""C:\a b\\""#),
            (r#"back\"slash"#, r#""back\\\"slash""#),
            (r"no\quote", r"no\quote"),
        ];
        for (argument, quoted) in cases {
            assert_eq!(quote(argument), quoted, "{argument:?}");
        }
    }

    #[test]
    fn a_task_becomes_a_link_with_its_quoted_arguments() {
        let task = JumpListTask::new("New note")
            .argument("--new")
            .argument("my notes")
            .description("Start a note");
        assert_eq!(
            links(&[task]),
            [TaskLink {
                title: "New note".to_owned(),
                arguments: "--new \"my notes\"".to_owned(),
                description: Some("Start a note".to_owned()),
            }]
        );
    }
}
