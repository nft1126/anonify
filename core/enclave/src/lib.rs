#![crate_name = "anonifyenclave"]
#![crate_type = "staticlib"]

#![cfg_attr(not(target_env = "sgx"), no_std)]
#![cfg_attr(target_env = "sgx", feature(rustc_private))]

#[cfg(not(target_env = "sgx"))]
#[macro_use]
extern crate sgx_tstd as std;
#[macro_use]
extern crate lazy_static;

use sgx_types::*;
use sgx_tse::*;
use anonify_types::*;
use ed25519_dalek::{PublicKey, Signature};
use crate::kvs::{MemoryKVS, SigVerificationKVS, MEMORY_DB};
use crate::state::UserState;
use crate::stf::Value;

mod crypto;
mod state;
mod error;
mod kvs;
mod auto_ffi;
mod sealing;
mod stf;

#[no_mangle]
pub unsafe extern "C" fn ecall_get_state(
    sig: &Sig,
    pubkey: &PubKey,
    msg: &Msg, // 32 bytes randomness for avoiding replay attacks.
    mut state: u64, // Currently, status is just value.
) -> sgx_status_t {
    let sig = Signature::from_bytes(&sig[..]).expect("Failed to read signatures.");
    let pubkey = PublicKey::from_bytes(&pubkey[..]).expect("Failed to read public key.");

    let db_value = MEMORY_DB.get(&msg[..], &sig, &pubkey).expect("Failed to get value from in-memory database.");
    let user_state = UserState::<Value, _>::get_state_from_db_value(db_value).expect("Failed to read db_value.");
    state = user_state.into_raw_u64();

    sgx_status_t::SGX_SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn ecall_write_state(
    ciphertext: &Ciphertext,
) -> sgx_status_t {

    sgx_status_t::SGX_SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn ecall_state_transition(
    sig: &Sig,
    target: &Address,
    value: u64,
    result: &mut TransitionResult,
) -> sgx_status_t {

    sgx_status_t::SGX_SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn ecall_contract_deploy(
    sig: &Sig,
    value: u64,
    result: &mut TransitionResult,
) -> sgx_status_t {

    sgx_status_t::SGX_SUCCESS
}


// extern "C" {
//     pub fn ocall_sgx_init_quote(
//         ret_val: *mut sgx_status_t,
//         ret_ti: *mut sgx_target_info_t,
//         ret_gid: *mut sgx_epid_group_id_t
//     ) -> sgx_status_t;

//     pub fn ocall_get_quote (
//         ret_val: *mut sgx_status_t,
//         p_sigrl: *const u8,
//         sigrl_len: u32,
//         p_report: *const sgx_report_t,
//         quote_type: sgx_quote_sign_type_t,
//         p_spid: *const sgx_spid_t,
//         p_nonce: *const sgx_quote_nonce_t,
//         p_qe_report: *mut sgx_report_t,
//         p_quote: *mut u8,
//         maxlen: u32,
//         p_quote_len: *mut u32
//     ) -> sgx_status_t;
// }

// TODO: Add sealed public key as extra data
// #[no_mangle]
// pub extern "C" fn ecall_get_registration_quote(
//     target_info: &sgx_target_info_t,
//     real_report: &mut sgx_report_t
// ) -> sgx_status_t {
//     let report = sgx_report_data_t::default();
//     if let Ok(r) = rsgx_create_report(&target_info, &report) {
//         *real_report = r;
//     }

//     sgx_status_t::SGX_SUCCESS
// }

pub mod tests {
    #[cfg(debug_assertions)]
    mod internal_tests {

    }
}
