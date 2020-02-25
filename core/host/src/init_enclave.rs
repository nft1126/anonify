use std::{
    fs, path::{Path, PathBuf},
    io::{Read, Write, BufReader, BufWriter},
};
use sgx_types::*;
use sgx_urts::SgxEnclave;
use crate::{
    constants::*,
    error::*,
};

pub struct EnclaveDir(PathBuf);

impl EnclaveDir {
    pub fn new() -> Self {
        let enclave_dir = dirs::home_dir()
            .expect("Cannot get enclave directory.")
            .join(ENCLAVE_DIR);

        if !enclave_dir.is_dir() {
            fs::create_dir_all(&enclave_dir)
                .expect("Cannot creat enclave directory.");
        }

        EnclaveDir(enclave_dir)
    }

    pub fn init_enclave(&self, is_debug: bool) -> anyhow::Result<SgxEnclave> {
        let token_file_path = self.get_token_file_path();
        let mut launch_token = Self::get_launch_token(&token_file_path)?;

        let mut launch_token_updated = 0;
        let enclave = Self::create_enclave(
            &mut launch_token,
            &mut launch_token_updated,
            is_debug
        ).expect("Failed to create enclave");

        // If launch token is updated, save it as token file.
        if launch_token_updated != 0 {
            Self::save_launch_token(&token_file_path, launch_token)?;
        }

        Ok(enclave)
    }

    fn get_token_file_path(&self) -> PathBuf {
        self.0.join(ENCLAVE_TOKEN)
    }

    fn get_launch_token<P: AsRef<Path>>(path: P) -> anyhow::Result<sgx_launch_token_t> {
        let mut buf = vec![];
        let mut res = [0u8; 1024];

        match fs::File::open(path) {
            Ok(f) => {
                let mut reader = BufReader::new(f);
                reader.read_to_end(&mut buf)?;
                assert_eq!(buf.len(), 1024);
                res.copy_from_slice(&buf[..]);
            },
            Err(_) => println!("No launch token file. Will create one."),
        }

        Ok(res)
    }

    fn save_launch_token<P: AsRef<Path>>(
        path: P,
        launch_token: sgx_launch_token_t,
    ) -> anyhow::Result<()> {
        let f = fs::File::create(path)?;
        let mut writer = BufWriter::new(f);
        writer.write_all(&launch_token[..])?;
        writer.flush()?;

        Ok(())
    }

    fn create_enclave(
        launch_token: &mut sgx_launch_token_t,
        launch_token_updated: &mut i32,
        is_debug: bool,
    ) -> SgxResult<SgxEnclave> {
        let debug = if is_debug {
            1 as i32
        } else {
            0 as i32
        };

        let mut misc_attr = sgx_misc_attribute_t {
            secs_attr: sgx_attributes_t {
                flags: 0,
                xfrm: 0,
            },
            misc_select: 0,
        };

        SgxEnclave::create(
            ENCLAVE_FILE,
            debug,
            launch_token,
            launch_token_updated,
            &mut misc_attr,
        )
    }
}

