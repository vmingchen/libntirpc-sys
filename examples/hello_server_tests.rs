use super::*;
use std::ffi::CStr;

struct Response {
    calls: u32,
    message: rpc_msg,
    greeting: Option<String>,
}

/// Capture a reply while its temporary result storage is still live.
///
/// # Safety
/// req must come from dispatch below, with rq_u1 pointing to its exclusive live
/// Response. A successful greeting must point to the synchronous string result
/// installed by hello_process; other replies must have matching discriminants.
unsafe extern "C" fn record_reply(req: *mut svc_req) -> xprt_stat {
    // SAFETY: dispatch owns req and Response and invokes hello_process
    // synchronously. Check the reply discriminants before reading union arms;
    // the success string is copied while its CString/pointer slot is live.
    unsafe {
        let response = &mut *((*req).rq_u1.cast::<Response>());
        response.calls += 1;
        response.message = (*req).rq_msg;
        let reply = &response.message.ru.RM_rmb;
        if reply.rp_stat == reply_stat_MSG_ACCEPTED
            && reply.ru.RP_ar.ar_stat == accept_stat_SUCCESS
            && response.message.cb_proc == 1
        {
            let result = reply.ru.RP_ar.ru.AR_results.where_;
            if !result.is_null() {
                let text = *result.cast::<*mut c_char>();
                response.greeting = Some(CStr::from_ptr(text).to_string_lossy().into_owned());
            }
        }
    }
    xprt_stat_XPRT_IDLE
}

fn dispatch(program: u32, version: u32, procedure: u32, flavor: i32, complete: bool) -> Response {
    // SAFETY: The zeroed values are C structs/unions of integers, raw pointers
    // and nullable callbacks. Every field needed by hello_process is populated
    // below. All pointees remain at stable stack addresses during the synchronous
    // call; aligned wire covers the chosen length. record_reply copies results
    // before their temporary storage goes away, and no worker is started.
    unsafe {
        let mut response = Response {
            calls: 0,
            message: std::mem::zeroed(),
            greeting: None,
        };
        let mut operations: svc_xprt_xp_ops = std::mem::zeroed();
        operations.xp_reply = Some(record_reply);
        let mut transport: SVCXPRT = std::mem::zeroed();
        transport.xp_ops = &mut operations;
        let mut wire = [3u32.to_be(), u32::from_ne_bytes(*b"abc\0")];
        let mut stream: XDR = std::mem::zeroed();
        xdrmem_ncreate(
            &mut stream,
            wire.as_mut_ptr().cast(),
            if complete { 8 } else { 4 },
            xdr_op_XDR_DECODE,
        );
        let mut request: svc_req = std::mem::zeroed();
        request.rq_xprt = &mut transport;
        request.rq_xdrs = &mut stream;
        request.rq_u1 = (&mut response as *mut Response).cast();
        request.rq_msg.rm_direction = msg_type_CALL;
        request.rq_msg.cb_prog = program;
        request.rq_msg.cb_vers = version;
        request.rq_msg.cb_proc = procedure;
        request.rq_msg.cb_cred.oa_flavor = flavor;
        hello_process(&mut request);
        // Results point into hello_process's former stack; the callback already
        // copied the greeting. Clear them before returning the captured header.
        if response.message.rm_direction == msg_type_REPLY
            && response.message.ru.RM_rmb.rp_stat == reply_stat_MSG_ACCEPTED
            && response.message.ru.RM_rmb.ru.RP_ar.ar_stat == accept_stat_SUCCESS
        {
            response.message.ru.RM_rmb.ru.RP_ar.ru.AR_results.proc_ = None;
            response.message.ru.RM_rmb.ru.RP_ar.ru.AR_results.where_ = std::ptr::null_mut();
        }
        response
    }
}

fn assert_accepted(response: &Response, expected: accept_stat) {
    assert_eq!(response.calls, 1);
    assert_eq!(response.message.rm_direction, msg_type_REPLY);
    assert_eq!(response.message.ru.RM_rmb.rp_stat, reply_stat_MSG_ACCEPTED);
    // SAFETY: The assertions above establish REPLY/MSG_ACCEPTED, selecting RP_ar.
    let status = unsafe { response.message.ru.RM_rmb.ru.RP_ar.ar_stat };
    assert_eq!(status, expected);
}

#[test]
fn greeting_procedure_returns_the_result_synchronously() {
    let response = dispatch(HELLO_PROG, HELLO_VERS, 1, AUTH_NONE as i32, true);
    assert_accepted(&response, accept_stat_SUCCESS);
    assert_eq!(response.greeting.as_deref(), Some("Hello, abc!"));
}

#[test]
fn malformed_arguments_report_garbage_args() {
    let response = dispatch(HELLO_PROG, HELLO_VERS, 1, AUTH_NONE as i32, false);
    assert_accepted(&response, accept_stat_GARBAGE_ARGS);
}

#[test]
fn unknown_procedure_reports_proc_unavailable() {
    let response = dispatch(HELLO_PROG, HELLO_VERS, 2, AUTH_NONE as i32, true);
    assert_accepted(&response, accept_stat_PROC_UNAVAIL);
}

#[test]
fn unknown_program_reports_program_unavailable() {
    let response = dispatch(HELLO_PROG + 1, HELLO_VERS, 1, AUTH_NONE as i32, true);
    assert_accepted(&response, accept_stat_PROG_UNAVAIL);
}

#[test]
fn unsupported_version_reports_the_supported_range() {
    let response = dispatch(HELLO_PROG, HELLO_VERS + 1, 1, AUTH_NONE as i32, true);
    assert_accepted(&response, accept_stat_PROG_MISMATCH);
    // SAFETY: assert_accepted verifies REPLY/MSG_ACCEPTED/PROG_MISMATCH,
    // which selects the initialized AR_versions arm.
    let versions = unsafe { response.message.ru.RM_rmb.ru.RP_ar.ru.AR_versions };
    assert_eq!(versions.low, HELLO_VERS);
    assert_eq!(versions.high, HELLO_VERS);
}

#[test]
fn null_procedure_does_not_decode_arguments() {
    let response = dispatch(HELLO_PROG, HELLO_VERS, 0, AUTH_NONE as i32, false);
    assert_accepted(&response, accept_stat_SUCCESS);
    assert!(response.greeting.is_none());
}

#[test]
fn rejected_authentication_produces_an_error_reply() {
    let response = dispatch(HELLO_PROG, HELLO_VERS, 1, 999, true);
    assert_eq!(response.calls, 1);
    assert_eq!(response.message.rm_direction, msg_type_REPLY);
    assert_eq!(response.message.ru.RM_rmb.rp_stat, reply_stat_MSG_DENIED);
    // SAFETY: The assertions above establish REPLY/MSG_DENIED, selecting RP_dr.
    let rejected = unsafe { response.message.ru.RM_rmb.ru.RP_dr };
    assert_eq!(rejected.rj_stat, reject_stat_AUTH_ERROR);
    // SAFETY: AUTH_ERROR above selects the initialized RJ_why arm.
    let reason = unsafe { rejected.ru.RJ_why };
    assert_eq!(reason, auth_stat_AUTH_REJECTEDCRED);
}
