use self::init_enclave::init_enclave;

mod init_enclave;
mod ocalls;
mod constants;

fn main() {
    let enclave = match init_enclave() {
        Ok(r) => {
            println!("[+] Init Enclave Successful {}!", r.geteid());
            r
        },
        Err(x) => {
            println!("[-] Init Enclave Failed {}!", x.as_str());
            return;
        },
    };

    println!("[+] Done!");

    enclave.destroy();
}
