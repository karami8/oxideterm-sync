// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

//! Touch ID Authentication (macOS only)
//!
//! Uses `LAContext` from the `LocalAuthentication` framework to authenticate
//! the user via Touch ID (or Apple Watch / device passcode as fallback).
//!
//! ## Why LAContext instead of SecAccessControl?
//!
//! `SecAccessControl` with biometric flags (`kSecAccessControlBiometryAny`)
//! requires the **Data Protection Keychain**, which in turn requires the app
//! binary to be **code-signed** with `keychain-access-groups` entitlement.
//! During `tauri dev` the binary is unsigned → `errSecMissingEntitlement (-34018)`.
//!
//! `LAContext.evaluatePolicy()` does **not** require any entitlements or code
//! signing. It simply asks the Secure Enclave to verify the user's identity.
//! The actual secrets remain in the regular `keyring` crate (cross-platform).
//!
//! ## Architecture
//!
//! - **Store**: regular `keyring` (no Touch ID – writing a key doesn't need auth)
//! - **Read**: `authenticate() → keyring.get()` (Touch ID gates access)
//! - **Delete / Exists**: regular `keyring` (no Touch ID needed)

use std::sync::mpsc;

use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{class, msg_send};
use objc2_foundation::{NSError, NSString};

// Link against the LocalAuthentication framework
#[link(name = "LocalAuthentication", kind = "framework")]
unsafe extern "C" {}

// ─── LAPolicy constants ─────────────────────────────────────────────────────

/// LAPolicyDeviceOwnerAuthenticationWithBiometrics = 1
/// Touch ID / Face ID only, no passcode fallback
const LA_POLICY_BIOMETRICS: i64 = 1;

/// LAPolicyDeviceOwnerAuthentication = 2
/// Touch ID / Face ID with device passcode fallback
const LA_POLICY_DEVICE_OWNER: i64 = 2;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Check whether biometric authentication (Touch ID) is available on this Mac.
///
/// Returns `true` if the device has Touch ID hardware and at least one
/// fingerprint is enrolled. Returns `false` on Macs without Touch ID
/// or if biometrics are locked out.
pub fn is_biometric_available() -> bool {
    unsafe {
        // LAContext *ctx = [[LAContext alloc] init];
        let cls = class!(LAContext);
        let ctx: *mut objc2::runtime::AnyObject = msg_send![cls, alloc];
        let ctx: *mut objc2::runtime::AnyObject = msg_send![ctx, init];

        if ctx.is_null() {
            return false;
        }

        // BOOL canEval = [ctx canEvaluatePolicy:LAPolicyDeviceOwnerAuthenticationWithBiometrics error:nil];
        let can_eval: Bool = msg_send![ctx, canEvaluatePolicy: LA_POLICY_BIOMETRICS, error: std::ptr::null_mut::<*mut NSError>()];

        can_eval.as_bool()
    }
}

/// Authenticate the user with Touch ID (biometrics + device passcode fallback).
///
/// Displays the system Touch ID dialog with the given `reason` string.
/// If the device has no Touch ID, falls back to macOS login password.
///
/// Returns `Ok(())` on successful authentication, `Err(message)` on failure
/// or if the user cancels.
///
/// ## Threading
///
/// `LAContext.evaluatePolicy` is asynchronous (provides a reply block).
/// This function blocks the current thread until the reply arrives via
/// `std::sync::mpsc::channel`. The Tauri command dispatcher runs commands
/// on a thread pool, so this is safe.
pub fn authenticate(reason: &str) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<Result<(), String>>();

    unsafe {
        // LAContext *ctx = [[LAContext alloc] init];
        let cls = class!(LAContext);
        let ctx: *mut objc2::runtime::AnyObject = msg_send![cls, alloc];
        let ctx: *mut objc2::runtime::AnyObject = msg_send![ctx, init];

        if ctx.is_null() {
            return Err("Failed to create LAContext".into());
        }

        // Check if device owner auth is available (biometrics + passcode fallback)
        let mut error_ptr: *mut NSError = std::ptr::null_mut();
        let can_eval: Bool =
            msg_send![ctx, canEvaluatePolicy: LA_POLICY_DEVICE_OWNER, error: &mut error_ptr];

        if !can_eval.as_bool() {
            // No biometrics and no passcode set — skip authentication
            let err_msg = if !error_ptr.is_null() {
                let err = &*error_ptr;
                let desc: Retained<NSString> = msg_send![err, localizedDescription];
                desc.to_string()
            } else {
                "Authentication not available".into()
            };
            tracing::debug!("LAContext: auth not available: {}", err_msg);
            // Return Ok — if the device can't do biometrics at all, skip the gate
            return Ok(());
        }

        let reason_ns = NSString::from_str(reason);

        // Build the completion block: ^(BOOL success, NSError *error)
        // SAFETY: The block captures `tx` and is called exactly once by LAContext.
        // The `error` pointer is valid for the duration of the block call.
        let block = block2::RcBlock::new(move |success: Bool, error: *mut NSError| {
            if success.as_bool() {
                let _ = tx.send(Ok(()));
            } else {
                let msg = if !error.is_null() {
                    let err = &*error;
                    let code: i64 = msg_send![err, code];
                    match code {
                        -2 => "Authentication canceled by user".to_string(), // LAErrorUserCancel
                        -4 => "Authentication canceled by system".to_string(), // LAErrorSystemCancel
                        -8 => "Authentication canceled by app".to_string(),    // LAErrorAppCancel
                        _ => {
                            let desc: Retained<NSString> = msg_send![err, localizedDescription];
                            desc.to_string()
                        }
                    }
                } else {
                    "Authentication failed".to_string()
                };
                let _ = tx.send(Err(msg));
            }
        });

        // [ctx evaluatePolicy:LAPolicyDeviceOwnerAuthentication
        //     localizedReason:reason
        //               reply:block];
        let _: () = msg_send![
            ctx,
            evaluatePolicy: LA_POLICY_DEVICE_OWNER,
            localizedReason: &*reason_ns,
            reply: &*block
        ];
    }

    // Block until the completion handler fires
    rx.recv()
        .unwrap_or(Err("Authentication channel closed".into()))
}
