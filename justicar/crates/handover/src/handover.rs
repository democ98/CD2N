use std::{collections::HashMap, fmt::format, time::Duration};

use crate::{utils, SgxError};
use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub struct HandoverHandler {
    ecdh_secret_key: Option<utils::EcdhSecretKey>,
    echd_public_key: Option<utils::EcdhPublicKey>,
    /// The last challenge create by this justicar
    handover_last_challenge: Option<HandoverChallenge>,

    /// The following content can be configue
    pub dev_mode: bool,
    pub pccs_url: String,
    pub ra_timeout: u64,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HandoverChallenge {
    pub sgx_target_info: Vec<u8>,
    pub block_number: u64,
    pub dev_mode: bool,
    pub nonce: [u8; 32],
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct ChallengeHandlerInfo {
    pub challenge: HandoverChallenge,
    pub sgx_local_report: Vec<u8>,
    pub ecdh_pubkey: [u8; 32],
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct HandoverChallengeResponse {
    challenge_handler: ChallengeHandlerInfo,
    attestation: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct EncryptedDataInfo {
    /// for key agreement
    pub ecdh_pubkey: [u8; 32],
    /// secret data encrypted by ecdh sharded key
    pub encrypted_data: Vec<u8>,
    /// IV nonce
    pub iv: [u8; 12],

    pub dev_mode: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
struct HandoverSecretData {
    encrypted_data_info: EncryptedDataInfo,
    attestation: Option<Vec<u8>>,
}

impl HandoverHandler {
    /// [Server]1st get challenge from old
    fn generate_challenge(&mut self, dev_mode: bool, block_number: u64) -> HandoverChallenge {
        let sgx_target_info = if dev_mode {
            vec![]
        } else {
            let my_target_info = crate::target_info().unwrap();
            crate::encode(&my_target_info).to_vec()
        };
        let challenge = HandoverChallenge {
            sgx_target_info,
            block_number,
            dev_mode: dev_mode,
            nonce: crate::utils::generate_random_byte::<32>(),
        };
        self.handover_last_challenge = Some(challenge.clone());
        challenge
    }

    ///[this]
    async fn handover_accept_challenge(
        &mut self,
        challenge: HandoverChallenge,
        ra: &impl RemoteAttestation,
    ) -> Result<HandoverChallengeResponse> {
        // do the secret exchange safely by using ECDH key exchange
        let (ecdh_secret_key, echd_public_key) = utils::gen_ecdh_key_pair();
        let dev_mode = challenge.dev_mode;
        self.ecdh_secret_key = Some(ecdh_secret_key);
        self.echd_public_key = Some(echd_public_key);

        // generate local attestation report to ensure the two justicar on same instance
        let sgx_local_report = if !dev_mode {
            let its_target_info = unsafe { crate::decode(&challenge.sgx_target_info)? };
            // the report data does not matter since we only care about the origin
            let report = crate::report(its_target_info, &[0; 64])?;
            crate::encode(&report).to_vec()
        } else {
            info!("create local attestation report in dev mode");
            vec![]
        };

        // generate remote attestation report,make the old justicar trust that the secret exchange with this one is credible
        let challenge_handler = ChallengeHandlerInfo {
            challenge,
            sgx_local_report,
            ecdh_pubkey: echd_public_key.to_bytes(),
        };

        let mut hasher = Sha256::new();
        hasher.update(
            serde_json::to_vec(&challenge_handler)
                .map_err(|e| SgxError::SerdeError(e.to_string()))?,
        );

        let handler_hash: [u8; 32] = hasher.finalize().into();

        let attestation = if !dev_mode {
            Some(ra.create_remote_attestation_report(
                &handler_hash,
                &self.pccs_url,
                Duration::from_secs(self.ra_timeout),
            ))
        } else {
            info!("dev mode does not need remote attestation");
            None
        };

        Ok(HandoverChallengeResponse {
            challenge_handler,
            attestation,
        })
    }

    /// [Server]Key Handover Server: Get worker key with RA report on challenge from another Ceseal
    async fn handover_start(
        &mut self,
        secret_data: Vec<u8>,
        response: HandoverChallengeResponse,
        ra: &impl RemoteAttestation,
        contract: &impl ExternalStatusGet,
    ) -> Result<HandoverSecretData> {
        let dev_mode = self.dev_mode;

        // 1. verify client RA report to ensure it's in sgx
        // this also ensure the message integrity
        let challenge_handler = response.challenge_handler;
        let attestation = if !dev_mode && response.attestation.is_some() {
            let mut hasher = Sha256::new();
            hasher.update(
                serde_json::to_vec(&challenge_handler)
                    .map_err(|e| SgxError::SerdeError(e.to_string()))?,
            );
            let payload_hash: [u8; 32] = hasher.finalize().into();
            let remote_attestation_report = response.attestation.unwrap();
            let pass = ra
                .verify_remote_attestation_report(&payload_hash, remote_attestation_report.clone());
            remote_attestation_report
        } else {
            info!("dev mod, client remote attestion report check skip");
            vec![]
        };

        // 2. verify challenge validity to prevent replay attack
        let challenge = challenge_handler.challenge;
        if !(self.handover_last_challenge.take().as_ref() == Some(&challenge)) {
            ///todo:return with error
            return Err(SgxError::HandoverFailed(
                "the challenge from client is invalid!".to_string(),
            )
            .into());
        }

        // 3. verify sgx local attestation report to ensure the handover justicar are on the same machine
        if !dev_mode {
            let recv_local_report = unsafe { crate::decode(&challenge_handler.sgx_local_report)? };
            crate::verify(recv_local_report)?;
        } else {
            info!("dev mode,client local attestation report check skip");
        }

        // 4. verify challenge block height and report timestamp
        // only challenge within 150 blocks (30 minutes) is accepted
        let current_block_number = contract.get_block_number();
        let challenge_height = challenge.block_number;
        if !(challenge_height <= current_block_number
            && current_block_number - challenge_height <= 150)
        {
            //todo:return with error
            return Err(SgxError::CryptoError("The challenge is expired!".to_string()).into());
        }
        // 5. check both side version time, never handover to old ceseal
        if !dev_mode {
            //server side
            let my_la_report = {
                // target_info and reportdata not important, we just need the report metadata
                let target_info = crate::target_info().expect("should not fail in SGX; qed.");
                crate::report(&target_info, &[0; 64])?
            };

            let server_mrenclave_list = contract.get_mrenclave_list();
            let server_mrenclave_record = server_mrenclave_list.get_key_value(
                &String::from_utf8(my_la_report.body.mr_enclave.m.to_vec())
                    .map_err(|e| SgxError::ParseError(e.to_string()))?,
            );
            let server_mrsigner_list = contract.get_mrsigner_list();
            let server_mrsigner_record = server_mrsigner_list.get_key_value(
                &String::from_utf8(my_la_report.body.mr_signer.m.to_vec())
                    .map_err(|e| SgxError::ParseError(e.to_string()))?,
            );
            if server_mrenclave_record.is_none() || server_mrsigner_record.is_none() {
                return Err(SgxError::InternalError(
                    "Server side justicar not allowed on contract!".to_string(),
                )
                .into());
            };
            if !(server_mrenclave_record.unwrap().1 == server_mrsigner_record.unwrap().1) {
                return Err(SgxError::InternalError(format!("Problem with the record loaded into the contract by the server side's justicar, blocknumber is different, please check the contract status! mrenclave:{:?}, mrsigner:{:?}",server_mrenclave_record.unwrap().0,server_mrsigner_record.unwrap().0) ).into());
            }

            //client side
            let (client_mrenclave, client_mrsigner) = ra
                .parse_mrenclave_and_mrsigner_from_attestation_report(attestation)
                .map_err(|e| {
                    SgxError::ParseError(format!(
                        "parse mrenclave and mrsigner from client ra report failed:{:?}",
                        e.to_string()
                    ))
                })?;

            let client_mrenclave_list = contract.get_mrenclave_list();
            let client_mrenclave_record = client_mrenclave_list.get_key_value(&client_mrenclave);
            let client_mrsigner_list = contract.get_mrsigner_list();
            let client_mrsigner_record = client_mrsigner_list.get_key_value(&client_mrsigner);
            if client_mrenclave_record.is_none() || client_mrsigner_record.is_none() {
                return Err(SgxError::InternalError(
                    "Server side justicar not allowed on contract!".to_string(),
                )
                .into());
            };
            if !(client_mrenclave_record.unwrap().1 == client_mrsigner_record.unwrap().1) {
                return Err(SgxError::InternalError(format!("Problem with the record loaded into the contract by the client side's justicar, blocknumber is different, please check the contract! mrenclave:{:?}, mrsigner:{:?}",client_mrenclave_record.unwrap().0,client_mrsigner_record.unwrap().0) ).into());
            }

            if server_mrenclave_record.unwrap().1 >= client_mrenclave_record.unwrap().1 {
                return Err(SgxError::HandoverFailed(
                    "The version of justicar on the server is later than that on the client"
                        .to_string(),
                )
                .into());
            }
        } else {
            info!("dev mod,client justicar blocknumber check skip");
        }

        // 6. Key exchange using remote attestation and ECDH
        let ecdh_pubkey = challenge_handler.ecdh_pubkey;
        let iv = utils::generate_random_byte::<12>();
        let (my_ecdh_secret_key, my_echd_public_key) = utils::gen_ecdh_key_pair();
        let client_ecdh_public_key =
            utils::convert_bytes_to_ecdh_public_key(challenge_handler.ecdh_pubkey.clone());
        let shared_secret_key =
            utils::echd_key_agreement(my_ecdh_secret_key, client_ecdh_public_key);

        let encrypted_data =
            utils::encrypt_secret_with_shared_key(&secret_data, &shared_secret_key, &iv)?;

        let encrypted_data_info = EncryptedDataInfo {
            ecdh_pubkey,
            encrypted_data,
            iv,
            dev_mode,
        };

        let mut hasher = Sha256::new();
        hasher.update(
            serde_json::to_vec(&encrypted_data_info)
                .map_err(|e| SgxError::SerdeError(e.to_string()))?,
        );
        let encrypted_data_info_hash: [u8; 32] = hasher.finalize().into();

        let attestation = if !dev_mode {
            Some(ra.create_remote_attestation_report(
                &encrypted_data_info_hash,
                &self.pccs_url,
                Duration::from_secs(self.ra_timeout),
            ))
        } else {
            info!("dev mod ,server remote attestion report check skip");
            None
        };

        Ok(HandoverSecretData {
            encrypted_data_info,
            attestation,
        })
    }
}

pub trait RemoteAttestation {
    fn create_remote_attestation_report(
        &self,
        payload: &[u8],
        pccs_url: &str,
        ra_timeout: Duration,
    ) -> Vec<u8>;

    ///Only verify the legitimacy of the report and do not make any business judgments.
    ///Of course, you can do so if you want.
    fn verify_remote_attestation_report(&self, payload: &[u8], attestation_report: Vec<u8>)
        -> bool;

    fn parse_mrenclave_and_mrsigner_from_attestation_report(
        &self,
        attestation_report: Vec<u8>,
    ) -> Result<(String, String)>;
}

pub trait ExternalStatusGet {
    fn get_block_number(&self) -> u64;
    fn get_mrenclave_list(&self) -> HashMap<String, u64>;
    fn get_mrsigner_list(&self) -> HashMap<String, u64>;
}
