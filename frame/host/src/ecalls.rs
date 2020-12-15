use crate::error::{FrameHostError, Result};
use codec::{Decode, Encode};
use frame_common::{EcallInput, EcallOutput};
use frame_types::EnclaveStatus;
use sgx_types::{sgx_enclave_id_t, sgx_status_t};

extern "C" {
    fn ecall_entry_point(
        eid: sgx_enclave_id_t,
        retval: *mut EnclaveStatus,
        cmd: u32,
        in_buf: *mut u8,
        in_len: usize,
        out_buf: *mut u8,
        out_max: usize,
        out_len: &mut usize,
    ) -> sgx_status_t;
}

pub struct EnclaveConnector {
    eid: sgx_enclave_id_t,
    output_max_len: usize,
}

impl EnclaveConnector {
    pub fn new(eid: sgx_enclave_id_t, output_max_len: usize) -> Self {
        EnclaveConnector {
            eid,
            output_max_len,
        }
    }

    pub fn invoke_ecall<E, D>(&self, cmd: u32, input: E) -> Result<D>
    where
        E: Encode + EcallInput,
        D: Decode + EcallOutput,
    {
        let input_payload = input.encode();
        let result = self.inner_invoke_ecall(cmd, input_payload)?;
        let response = D::decode(&mut &result[..])?;

        Ok(response)
    }

    fn inner_invoke_ecall(&self, cmd: u32, mut input: Vec<u8>) -> Result<Vec<u8>> {
        let input_ptr = input.as_mut_ptr();
        let input_len = input.len();
        let output_max = self.output_max_len;
        let mut output_len = output_max;
        let mut output_buf = Vec::with_capacity(output_max);
        let output_ptr = output_buf.as_mut_ptr();

        let mut ret = EnclaveStatus::default();

        let t0 = std::time::SystemTime::now();
        let status = unsafe {
            ecall_entry_point(
                self.eid,
                &mut ret,
                cmd,
                input_ptr,
                input_len,
                output_ptr,
                output_max,
                &mut output_len,
            )
        };
        let t1 = std::time::SystemTime::now();
        println!("t0: {:?}", t0);
        println!("t1: {:?}", t1);

        if status != sgx_status_t::SGX_SUCCESS {
            return Err(FrameHostError::SgxStatus {
                status,
                function: "ecall_entry_point(SgxStatus)",
                cmd,
            });
        }
        if ret.is_err() {
            return Err(FrameHostError::EnclaveError {
                status: ret,
                function: "ecall_entry_point",
                cmd,
            });
        }
        assert!(output_len < output_max);

        unsafe {
            output_buf.set_len(output_len);
        }

        Ok(output_buf)
    }
}
