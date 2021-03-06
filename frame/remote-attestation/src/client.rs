use anyhow::{anyhow, bail, ensure, Result};
use http_req::{
    request::{Method, Request},
    response::{Headers, Response},
    uri::Uri,
};
use log::debug;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{io::Write, prelude::v1::*, str, string::String, time::SystemTime};

type SignatureAlgorithms = &'static [&'static webpki::SignatureAlgorithm];
static SUPPORTED_SIG_ALGS: SignatureAlgorithms = &[
    &webpki::ECDSA_P256_SHA256,
    &webpki::ECDSA_P256_SHA384,
    &webpki::ECDSA_P384_SHA256,
    &webpki::ECDSA_P384_SHA384,
    &webpki::RSA_PSS_2048_8192_SHA256_LEGACY_KEY,
    &webpki::RSA_PSS_2048_8192_SHA384_LEGACY_KEY,
    &webpki::RSA_PSS_2048_8192_SHA512_LEGACY_KEY,
    &webpki::RSA_PKCS1_2048_8192_SHA256,
    &webpki::RSA_PKCS1_2048_8192_SHA384,
    &webpki::RSA_PKCS1_2048_8192_SHA512,
    &webpki::RSA_PKCS1_3072_8192_SHA384,
];

/// A client for remote attestation with IAS
pub struct RAClient<'a> {
    request: Request<'a>,
    host: String,
}

impl<'a> RAClient<'a> {
    pub fn new(uri: &'a Uri) -> Self {
        let host = uri.host_header().expect("Not found host in the uri");

        RAClient {
            request: Request::new(&uri),
            host,
        }
    }

    /// Sets IAS API KEY to header.
    pub fn ias_apikey_header_mut(&mut self, ias_api_key: &str) -> &mut Self {
        let mut headers = Headers::new();
        headers.insert("HOST", &self.host);
        headers.insert("Ocp-Apim-Subscription-Key", ias_api_key);
        headers.insert("Connection", "close");
        self.request.headers(headers);
        self.request.method(Method::POST);

        self
    }

    /// Sets the body to the JSON serialization of the passed value, and
    /// also sets the `Content-Type: application/json` header.
    pub fn quote_body_mut(&'a mut self, body: &'a [u8]) -> &mut Self {
        let len = body.len().to_string();
        self.request.header("Content-Type", "application/json");
        self.request.header("Content-Length", &len);
        self.request.body(&body);

        self
    }

    pub fn send<T: Write>(&self, writer: &mut T) -> Result<Response> {
        self.request
            .send(writer)
            .map_err(|e| anyhow!("{:?}", e))
            .map_err(Into::into)
    }
}

/// A response from IAS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestedReport {
    /// A report returned from Attestation Service
    report: Vec<u8>,
    /// A signature of the report
    report_sig: Vec<u8>,
    /// A certificate of the signing key of the signature
    report_cert: Vec<u8>,
}

impl AttestedReport {
    pub(crate) fn from_response(body: Vec<u8>, resp: Response) -> Result<Self> {
        debug!("RA response: {:?}", resp);

        let headers = resp.headers();
        let sig = headers
            .get("X-IASReport-Signature")
            .ok_or_else(|| anyhow!("Not found X-IASReport-Signature header"))?;
        let report_sig = base64::decode(&sig)?;

        let cert = headers
            .get("X-IASReport-Signing-Certificate")
            .ok_or_else(|| anyhow!("Not found X-IASReport-Signing-Certificate"))?
            .replace("%0A", "");
        let report_cert = percent_decode(cert)?;

        Ok(AttestedReport {
            report: body,
            report_sig,
            report_cert,
        })
    }

    /// Verify that
    /// 1. TLS server certificate
    /// 2. report's signature
    /// 3. report's version
    /// 4. quote status
    #[must_use]
    pub fn verify_attested_report(self, root_cert: Vec<u8>) -> Result<Self> {
        let now_func = webpki::Time::try_from(SystemTime::now())?;

        let mut root_store = rustls::RootCertStore::empty();
        root_store.add(&rustls::Certificate(root_cert.clone()))?;

        let trust_anchors: Vec<webpki::TrustAnchor> = root_store
            .roots
            .iter()
            .map(|cert| cert.to_trust_anchor())
            .collect();

        let mut chain: Vec<&[u8]> = Vec::new();
        chain.push(&root_cert);

        let report_cert = webpki::EndEntityCert::from(&self.report_cert)?;

        report_cert.verify_is_valid_tls_server_cert(
            SUPPORTED_SIG_ALGS,
            &webpki::TLSServerTrustAnchors(&trust_anchors),
            &chain,
            now_func,
        )?;

        report_cert.verify_signature(
            &webpki::RSA_PKCS1_2048_8192_SHA256,
            &self.report,
            &self.report_sig,
        )?;

        let report = serde_json::from_slice(&self.report)?;
        Self::verify_version(&report)?;
        Self::verify_quote_status(&report)?;

        Ok(self)
    }

    pub fn get_quote_body(&self) -> Result<Vec<u8>> {
        let report: Value = serde_json::from_slice(&self.report)?;
        let encoded_quote = report["isvEnclaveQuoteBody"]
            .as_str()
            .ok_or_else(|| anyhow!("Invalid isvEnclaveQuoteBody"))?;
        base64::decode(encoded_quote).map_err(Into::into)
    }

    pub fn report(&self) -> &[u8] {
        &self.report
    }

    pub fn report_sig(&self) -> &[u8] {
        &self.report_sig
    }

    pub fn report_cert(&self) -> &[u8] {
        &self.report_cert
    }

    /// Verify API version is supported
    fn verify_version(report: &Value) -> Result<()> {
        let version = report["version"]
            .as_u64()
            .ok_or_else(|| anyhow!("The Remote Attestation API version is not valid"))?;
        ensure!(
            version == 3,
            "The Remote Attestation API version is not supported"
        );
        Ok(())
    }

    /// Verify the quote status included the attestation report is OK
    fn verify_quote_status(report: &Value) -> Result<()> {
        if let Value::String(quote_status) = &report["isvEnclaveQuoteStatus"] {
            match quote_status.as_ref() {
                "OK" => Ok(()),
                "GROUP_OUT_OF_DATE" => {
                    println!("Enclave Quote Status: GROUP_OUT_OF_DATE");
                    Ok(())
                }
                _ => bail!("Invalid Enclave Quote Status: {}", quote_status),
            }
        } else {
            bail!("Failed to fetch isvEnclaveQuoteStatus from attestation report");
        }
    }
}

fn percent_decode(orig: String) -> Result<Vec<u8>> {
    let v: Vec<&str> = orig.split('%').collect();
    ensure!(!v.is_empty(), "Certificate is blank");
    let mut ret = String::new();
    ret.push_str(v[0]);
    if v.len() > 1 {
        for s in v[1..].iter() {
            ret.push(u8::from_str_radix(&s[0..2], 16)? as char);
            ret.push_str(&s[2..]);
        }
    }
    let v: Vec<&str> = ret.split("-----").collect();
    base64::decode(v[2]).map_err(Into::into)
}
