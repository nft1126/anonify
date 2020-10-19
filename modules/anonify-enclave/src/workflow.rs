use crate::instructions::Instructions;
use anonify_io_types::*;
use anyhow::{anyhow, Result};
use codec::{Decode, Encode};
use frame_common::{crypto::Sha256, state_types::StateType, traits::*};
use frame_enclave::EnclaveEngine;
use frame_runtime::traits::*;
use frame_treekem::handshake::HandshakeParams;
use remote_attestation::RAService;
use std::{env, marker::PhantomData};

#[derive(Debug, Clone)]
pub struct Instruction<AP: AccessPolicy> {
    phantom: PhantomData<AP>,
}

impl<AP: AccessPolicy> EnclaveEngine for Instruction<AP> {
    type EI = input::Instruction<AP>;
    type EO = output::Instruction;

    fn eval_policy(ecall_input: &Self::EI) -> anyhow::Result<()> {
        ecall_input.access_policy().verify()
    }

    fn handle<R, C>(
        mut ecall_input: Self::EI,
        enclave_context: &C,
        max_mem_size: usize,
    ) -> Result<Self::EO>
    where
        R: RuntimeExecutor<C, S = StateType>,
        C: ContextOps<S = StateType> + Clone,
    {
        let account_id = ecall_input.access_policy().into_account_id();

        let group_key = &mut *enclave_context.write_group_key();
        let ciphertext = Instructions::<R, C>::new(
            ecall_input.call_id,
            ecall_input.state.as_mut_bytes(),
            account_id,
        )?
        .encrypt(group_key, max_mem_size)?;

        let msg = Sha256::hash(&ciphertext.encode());
        let enclave_sig = enclave_context.sign(msg.as_bytes())?;
        let roster_idx = ciphertext.roster_idx() as usize;
        let instruction_output = output::Instruction::new(ciphertext, enclave_sig);

        enclave_context.set_notification(account_id);
        // ratchet sender's app keychain per tx.
        group_key.sender_ratchet(roster_idx)?;

        Ok(instruction_output)
    }
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct InsertCiphertext;

impl EnclaveEngine for InsertCiphertext {
    type EI = input::InsertCiphertext;
    type EO = output::ReturnUpdatedState;

    fn handle<R, C>(
        ecall_input: Self::EI,
        enclave_context: &C,
        _max_mem_size: usize,
    ) -> Result<Self::EO>
    where
        R: RuntimeExecutor<C, S = StateType>,
        C: ContextOps<S = StateType> + Clone,
    {
        let group_key = &mut *enclave_context.write_group_key();
        let roster_idx = ecall_input.ciphertext().roster_idx() as usize;

        // Since the sender's keychain has already ratcheted,
        // even if an error occurs in the state transition, the receiver's keychain also ratchet.
        // `receiver_ratchet` fails if
        //   1. Roster index is out of range of the keychain
        //   2. error occurs in HKDF
        //   3. the generation is over u32::MAX
        // In addition to these, `sync_ratchet` fails even if the receiver generation is larger than that of the sender
        // So if you run `sync_ratchet` first,
        // it will either succeed or both fail for the mutable `app_keychain`, so it will be atomic.
        group_key.sync_ratchet(roster_idx)?;
        group_key.receiver_ratchet(roster_idx)?;

        // Even if an error occurs in the state transition logic here, there is no problem because the state of `app_keychain` is consistent.
        let iter_op = Instructions::<R, C>::state_transition(
            enclave_context.clone(),
            ecall_input.ciphertext(),
            group_key,
        )?;
        let mut output = output::ReturnUpdatedState::default();

        if let Some(updated_state_iter) = iter_op {
            if let Some(updated_state) = enclave_context.update_state(updated_state_iter) {
                output.update(updated_state);
            }
        }

        Ok(output)
    }
}

#[derive(Debug, Clone)]
pub struct InsertHandshake;

impl EnclaveEngine for InsertHandshake {
    type EI = input::InsertHandshake;
    type EO = output::Empty;

    fn handle<R, C>(
        ecall_input: Self::EI,
        enclave_context: &C,
        _max_mem_size: usize,
    ) -> Result<Self::EO>
    where
        R: RuntimeExecutor<C, S = StateType>,
        C: ContextOps<S = StateType> + Clone,
    {
        let group_key = &mut *enclave_context.write_group_key();
        let handshake = HandshakeParams::decode(&mut &ecall_input.handshake().handshake()[..])
            .map_err(|_| anyhow!("HandshakeParams::decode Error"))?;

        group_key.process_handshake(&handshake)?;

        Ok(output::Empty::default())
    }
}

#[derive(Debug, Clone)]
pub struct GetState<AP: AccessPolicy> {
    phantom: PhantomData<AP>,
}

impl<AP: AccessPolicy> EnclaveEngine for GetState<AP> {
    type EI = input::GetState<AP>;
    type EO = output::ReturnState;

    fn eval_policy(ecall_input: &Self::EI) -> anyhow::Result<()> {
        ecall_input.access_policy().verify()
    }

    fn handle<R, C>(
        ecall_input: Self::EI,
        enclave_context: &C,
        _max_mem_size: usize,
    ) -> Result<Self::EO>
    where
        R: RuntimeExecutor<C, S = StateType>,
        C: ContextOps<S = StateType> + Clone,
    {
        let account_id = ecall_input.access_policy().into_account_id();
        let user_state = enclave_context.get_state(account_id, ecall_input.mem_id());

        Ok(output::ReturnState::new(user_state))
    }
}

#[derive(Debug, Clone)]
pub struct CallJoinGroup;

impl EnclaveEngine for CallJoinGroup {
    type EI = input::CallJoinGroup;
    type EO = output::ReturnJoinGroup;

    fn handle<R, C>(
        _ecall_input: Self::EI,
        enclave_context: &C,
        _max_mem_size: usize,
    ) -> Result<Self::EO>
    where
        R: RuntimeExecutor<C, S = StateType>,
        C: ContextOps<S = StateType> + Clone,
    {
        let quote = enclave_context.quote()?;
        let ias_url = env::var("IAS_URL")?;
        let sub_key = env::var("SUB_KEY")?;
        let (report, report_sig) =
            RAService::remote_attestation(ias_url.as_str(), sub_key.as_str(), &quote)?;
        let mrenclave_ver = enclave_context.mrenclave_ver();
        let group_key = &*enclave_context.read_group_key();
        let (export_handshake, export_path_secret) = group_key.create_handshake()?;

        Ok(output::ReturnJoinGroup::new(
            report.into_vec(),
            report_sig.into_vec(),
            export_handshake.encode(),
            mrenclave_ver,
            export_handshake.roster_idx(),
            export_path_secret,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct CallHandshake;

impl EnclaveEngine for CallHandshake {
    type EI = input::CallHandshake;
    type EO = output::ReturnHandshake;

    fn handle<R, C>(
        _ecall_input: Self::EI,
        enclave_context: &C,
        _max_mem_size: usize,
    ) -> Result<Self::EO>
    where
        R: RuntimeExecutor<C, S = StateType>,
        C: ContextOps<S = StateType> + Clone,
    {
        let group_key = &*enclave_context.read_group_key();
        let (export_handshake, export_path_secret) = group_key.create_handshake()?;
        let roster_idx = export_handshake.roster_idx();
        let msg = Sha256::hash_with_u32(&export_handshake.encode(), roster_idx);
        let enclave_sig = enclave_context.sign(msg.as_bytes())?;

        Ok(output::ReturnHandshake::new(
            export_handshake,
            export_path_secret,
            enclave_sig,
            roster_idx,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct RegisterNotification<AP: AccessPolicy> {
    phantom: PhantomData<AP>,
}

impl<AP: AccessPolicy> EnclaveEngine for RegisterNotification<AP> {
    type EI = input::RegisterNotification<AP>;
    type EO = output::Empty;

    fn eval_policy(ecall_input: &Self::EI) -> anyhow::Result<()> {
        ecall_input.access_policy().verify()
    }

    fn handle<R, C>(
        ecall_input: Self::EI,
        enclave_context: &C,
        _max_mem_size: usize,
    ) -> Result<Self::EO>
    where
        R: RuntimeExecutor<C, S = StateType>,
        C: ContextOps<S = StateType> + Clone,
    {
        let account_id = ecall_input.access_policy().into_account_id();
        enclave_context.set_notification(account_id);

        Ok(output::Empty::default())
    }
}
