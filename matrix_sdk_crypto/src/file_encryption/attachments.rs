// Copyright 2020 The Matrix.org Foundation C.I.C.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{collections::BTreeMap, io::Read};

use serde::{Deserialize, Serialize};

use matrix_sdk_common::events::room::JsonWebKey;

use getrandom::getrandom;

use aes_ctr::{
    stream_cipher::{NewStreamCipher, SyncStreamCipher},
    Aes256Ctr,
};
use sha2::{Digest, Sha256};

use super::{decode, decode_url_safe, encode, encode_url_safe};

const IV_SIZE: usize = 16;
const KEY_SIZE: usize = 32;
const VERSION: u8 = 1;

#[allow(missing_docs)]
pub struct AttachmentDecryptor<'a, R: 'a + Read> {
    inner_reader: &'a mut R,
    expected_hash: Vec<u8>,
    sha: Sha256,
    aes: Aes256Ctr,
}

impl<'a, R: Read> Read for AttachmentDecryptor<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read_bytes = self.inner_reader.read(buf)?;

        if read_bytes == 0 {
            let hash = self.sha.finalize_reset();
            if hash.as_slice() == self.expected_hash.as_slice() {
                Ok(0)
            } else {
                panic!("INVALID HASH");
            }
        } else {
            self.sha.update(&buf[0..read_bytes]);
            self.aes.apply_keystream(&mut buf[0..read_bytes]);

            Ok(read_bytes)
        }
    }
}

impl<'a, R: Read + 'a> AttachmentDecryptor<'a, R> {
    #[allow(missing_docs)]
    fn new(input: &'a mut R, info: EncryptionInfo) -> AttachmentDecryptor<'a, R> {
        // TODO check the version
        let hash = decode(info.hashes.get("sha256").unwrap()).unwrap();
        // TODO Use zeroizing here.
        let key = decode_url_safe(info.web_key.k).unwrap();
        let iv = decode(info.iv).unwrap();

        let sha = Sha256::default();
        let aes = Aes256Ctr::new_var(&key, &iv).unwrap();

        AttachmentDecryptor {
            inner_reader: input,
            expected_hash: hash,
            sha,
            aes,
        }
    }
}

#[allow(missing_docs)]
#[derive(Debug)]
pub struct AttachmentEncryptor<'a, R: Read + 'a> {
    finished: bool,
    inner_reader: &'a mut R,
    web_key: JsonWebKey,
    iv: String,
    hashes: BTreeMap<String, String>,
    aes: Aes256Ctr,
    sha: Sha256,
}

impl<'a, R: Read + 'a> Read for AttachmentEncryptor<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read_bytes = self.inner_reader.read(buf)?;

        if read_bytes == 0 {
            let hash = self.sha.finalize_reset();
            self.hashes
                .entry("sha256".to_owned())
                .or_insert_with(|| encode(hash));
            Ok(0)
        } else {
            self.aes.apply_keystream(&mut buf[0..read_bytes]);
            self.sha.update(&buf[0..read_bytes]);

            Ok(read_bytes)
        }
    }
}

impl<'a, R: Read + 'a> AttachmentEncryptor<'a, R> {
    #[allow(missing_docs)]
    pub fn new(reader: &'a mut R) -> Self {
        // TODO Use zeroizing here.
        let mut key = [0u8; KEY_SIZE];
        let mut iv = [0u8; IV_SIZE];

        getrandom(&mut key).expect("Can't generate randomness");
        // Only populate the the first 8 bits with randomness, the rest is 0
        // initialized.
        getrandom(&mut iv[0..8]).expect("Can't generate randomness");

        let web_key = JsonWebKey {
            kty: "oct".to_owned(),
            key_ops: vec!["encrypt".to_owned(), "decrypt".to_owned()],
            alg: "A256CTR".to_owned(),
            k: encode_url_safe(key),
            ext: true,
        };
        let encoded_iv = encode(iv);

        let aes = Aes256Ctr::new_var(&key, &iv).expect("Cannot create AES encryption object.");

        AttachmentEncryptor {
            finished: false,
            inner_reader: reader,
            iv: encoded_iv,
            web_key,
            hashes: BTreeMap::new(),
            aes,
            sha: Sha256::default(),
        }
    }

    #[allow(missing_docs)]
    pub fn finish(mut self) -> EncryptionInfo {
        let hash = self.sha.finalize();
        self.hashes
            .entry("sha256".to_owned())
            .or_insert_with(|| encode(hash));

        EncryptionInfo {
            version: "v2".to_string(),
            hashes: self.hashes,
            iv: self.iv,
            web_key: self.web_key,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptionInfo {
    #[serde(rename = "v")]
    pub version: String,
    pub web_key: JsonWebKey,
    pub iv: String,
    pub hashes: BTreeMap<String, String>,
}

#[cfg(test)]
mod test {
    use super::{AttachmentDecryptor, AttachmentEncryptor, EncryptionInfo};
    use serde_json::json;
    use std::io::{Cursor, Read};

    const EXAMPLE_DATA: &[u8] = &[
        179, 154, 118, 127, 186, 127, 110, 33, 203, 33, 33, 134, 67, 100, 173, 46, 235, 27, 215,
        172, 36, 26, 75, 47, 33, 160,
    ];

    fn example_key() -> EncryptionInfo {
        let info = json!({
            "v": "v2",
            "web_key": {
                "kty": "oct",
                "alg": "A256CTR",
                "ext": true,
                "k": "Voq2nkPme_x8no5-Tjq_laDAdxE6iDbxnlQXxwFPgE4",
                "key_ops": ["encrypt", "decrypt"]
            },
            "iv": "i0DovxYdJEcAAAAAAAAAAA",
            "hashes": {
                "sha256": "ANdt819a8bZl4jKy3Z+jcqtiNICa2y0AW4BBJ/iQRAU"
            }
        });

        serde_json::from_value(info).unwrap()
    }

    #[test]
    fn encrypt_decrypt_cycle() {
        let data = "Hello world".to_owned();
        let mut cursor = Cursor::new(data.clone());

        let mut encryptor = AttachmentEncryptor::new(&mut cursor);

        let mut encrypted = Vec::new();

        encryptor.read_to_end(&mut encrypted).unwrap();
        let key = encryptor.finish();
        assert_ne!(encrypted.as_slice(), data.as_bytes());

        let mut cursor = Cursor::new(encrypted);
        let mut decryptor = AttachmentDecryptor::new(&mut cursor, key);
        let mut decrypted_data = Vec::new();

        decryptor.read_to_end(&mut decrypted_data).unwrap();

        let decrypted = String::from_utf8(decrypted_data).unwrap();

        assert_eq!(data, decrypted);
    }

    #[test]
    fn real_decrypt() {
        let mut cursor = Cursor::new(EXAMPLE_DATA.to_vec());
        let key = example_key();

        let mut decryptor = AttachmentDecryptor::new(&mut cursor, key);
        let mut decrypted_data = Vec::new();

        decryptor.read_to_end(&mut decrypted_data).unwrap();
        let decrypted = String::from_utf8(decrypted_data).unwrap();

        assert_eq!("It's a secret to everybody", decrypted);
    }
}