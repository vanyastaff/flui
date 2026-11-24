//! The client vocabulary of ADR-0400 §1 and §7: what a call's failure says to a report, what a
//! test's end asks by default, and that a passed check is never an unpassed one.

use flui_protocol::audit::AuditRule;
use flui_protocol::client::{CallError, EndChecks};
use flui_protocol::error::{ErrorCode, ErrorObject};
use flui_protocol::report::{Check, CheckFailure, Outcome, UnpassedCheck};

#[test]
fn a_call_s_failure_is_cannot_verify_only_when_the_target_lacks_something() {
    let mut lacking = ErrorObject::new(ErrorCode::NotSupported, "no OS lane");
    lacking.capability = Some("os_input".to_owned());
    let table = [
        (
            "lacks a named capability",
            CallError::failed(lacking),
            Some("os_input"),
        ),
        (
            "lacks an unnamed one",
            CallError::failed(ErrorObject::new(ErrorCode::NotSupported, "no")),
            Some("unnamed"),
        ),
        (
            "refused",
            CallError::failed(ErrorObject::new(ErrorCode::NotFound, "nothing matched")),
            None,
        ),
        (
            "unanswered",
            CallError::Unanswered {
                may_have_run: true,
                released: false,
            },
            None,
        ),
    ];
    for (case, error, capability) in table {
        assert_eq!(error.is_cannot_verify(), capability.is_some(), "{case}");
        assert_eq!(error.missing_capability().as_deref(), capability, "{case}");
        let check = Check::from_result::<(), _>(case, &Err(error));
        match capability {
            Some(lacking) => assert!(
                matches!(
                    &check.outcome,
                    Outcome::CannotVerify { capability: Some(named), .. } if named == lacking
                ),
                "{case}: {check:?}"
            ),
            None => assert!(
                matches!(check.outcome, Outcome::Failed { .. }),
                "{case}: {check:?}"
            ),
        }
    }
}

#[test]
fn a_test_s_end_asks_every_check_by_default() {
    let checks = EndChecks::default();
    assert!(checks.settle && checks.drop_ledger && checks.left_behind);
    assert_eq!(checks.audit, [AuditRule::Unnamed]);
    let none = EndChecks::none();
    assert!(!none.settle && !none.drop_ledger && !none.left_behind && none.audit.is_empty());
    assert_eq!(
        EndChecks::none()
            .settle(true)
            .audit(vec![AuditRule::TextContrast]),
        EndChecks::default()
            .drop_ledger(false)
            .left_behind(false)
            .audit(vec![AuditRule::TextContrast])
    );
}

#[test]
fn a_passed_check_is_never_unpassed() {
    assert_eq!(
        UnpassedCheck::try_from_check(Check::new("held", Outcome::Passed)),
        None
    );
    let failed = UnpassedCheck::failed("released", "a key stays down");
    assert_eq!(
        UnpassedCheck::try_from_check(failed.check().clone()),
        Some(failed.clone())
    );
    assert!(matches!(
        UnpassedCheck::cannot_verify("ledger", "no ledger", None)
            .into_check()
            .outcome,
        Outcome::CannotVerify { .. }
    ));
}
