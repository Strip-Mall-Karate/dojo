// Copyright (c) 2016-2017 Chef Software Inc. and/or applicable contributors
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

use std::{fs::File,
          io::{BufReader,
               Read}};

use blake2::{digest::consts::U32,
             Blake2b,
             Digest};

use crate::error::Result;

/// Keyless BLAKE2b with a 32-byte digest: libsodium's `crypto_generichash`
/// with its default size, which earlier releases of Dojo hashed with.
type Blake2b256 = Blake2b<U32>;

const BUF_SIZE: usize = 1024;

/// Calculate the BLAKE2b hash of a file, return as a hex string
/// digest size = 32 BYTES
/// NOTE: the hashing is keyless
pub fn hash_file<P>(filename: P) -> Result<String>
    where P: AsRef<std::path::Path>
{
    let file = File::open(filename.as_ref())?;
    let mut reader = BufReader::new(file);
    hash_reader(&mut reader)
}

pub fn hash_string(data: &str) -> String { hash_bytes(data.as_bytes()) }

pub fn hash_bytes(data: &[u8]) -> String { hex::encode(Blake2b256::digest(data)) }

pub fn hash_reader(reader: &mut BufReader<File>) -> Result<String> {
    let mut hasher = Blake2b256::new();
    let mut buf = [0u8; BUF_SIZE];
    loop {
        let bytes_read = reader.read(&mut buf)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buf[..bytes_read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod test {
    use super::{super::test_support::*,
                *};

    #[test]
    fn hash_file_working() {
        // The expected values were computed using the `b2sum` program from
        // https://github.com/dchest/b2sum using the the `-s=32` option. For example:
        //      b2sum -s=32 signme.dat

        let computed = hash_file(&fixture("signme.dat")).unwrap();
        let expected = "20590a52c4f00588c500328b16d466c982a26fabaa5fa4dcc83052dd0a84f233";
        assert_eq!(computed, expected);

        let computed = hash_file(&fixture("happyhumans-20160424223347.sig.key")).unwrap();
        let expected = "e966844bbc50b7a3a6d81e94dd38e27b92814b944095a8e55f1780921bfcfbe1";
        assert_eq!(computed, expected);

        let computed = hash_file(&fixture("happyhumans-20160424223347.pub")).unwrap();
        let expected = "b80c4f412f9a0a7727b6e6f115e1b5fa3bae79ad2fcf47f769ed4e42cfb12265";
        assert_eq!(computed, expected);
    }
}
