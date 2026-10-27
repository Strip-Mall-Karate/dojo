// Copyright (c) 2026 Strip Mall Karate Company
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

//! The libsodium constructions Dojo's keys, signatures and encrypted messages
//! have always used, implemented with RustCrypto. Every output is byte for
//! byte what libsodium produces, so keys, signed artifacts and encrypted
//! messages from earlier releases still read:
//!
//! - `sign`: Ed25519 in libsodium's combined mode, the signature followed by the message. A secret
//!   key is the 32-byte seed followed by the public key.
//! - `box_`: `crypto_box_easy`, X25519 with XSalsa20-Poly1305.
//! - `sealedbox`: `crypto_box_seal`, anonymous boxes.
//! - `secretbox`: `crypto_secretbox_easy`, XSalsa20-Poly1305.

use std::{convert::{TryFrom,
                    TryInto},
          fmt,
          ops::{Index,
                RangeFull}};

use zeroize::Zeroize;

/// Defines a fixed-size key or nonce with the surface the key code uses.
macro_rules! bytes_type {
    ($name:ident, $len:expr,secret) => {
        bytes_type!($name, $len);

        impl Drop for $name {
            fn drop(&mut self) { self.0.zeroize(); }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "(..)"))
            }
        }
    };
    ($name:ident, $len:expr,public) => {
        bytes_type!($name, $len);

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), hex::encode(self.0))
            }
        }
    };
    ($name:ident, $len:expr) => {
        #[derive(Clone, PartialEq, Eq)]
        pub struct $name(pub [u8; $len]);

        impl $name {
            pub const BYTES: usize = $len;

            /// The key or nonce in `bytes`, if it is exactly the right length.
            pub fn from_slice(bytes: &[u8]) -> Option<Self> {
                <[u8; $len]>::try_from(bytes).ok().map($name)
            }
        }

        impl AsRef<[u8]> for $name {
            fn as_ref(&self) -> &[u8] { &self.0 }
        }

        impl Index<RangeFull> for $name {
            type Output = [u8];

            fn index(&self, _: RangeFull) -> &[u8] { &self.0 }
        }
    };
}

/// `n` random bytes from the operating system.
pub fn randombytes(n: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).expect("the operating system provides random bytes");
    bytes
}

fn random_array<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("the operating system provides random bytes");
    bytes
}

/// Whether `a` and `b` are equal, in time that depends only on their length.
pub fn memcmp(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

pub mod sign {
    use ed25519_dalek::{Signature,
                        Signer,
                        SigningKey,
                        VerifyingKey};

    use super::*;

    bytes_type!(PublicKey, 32, public);
    bytes_type!(SecretKey, 64, secret);

    pub const SIGNATUREBYTES: usize = 64;

    pub fn gen_keypair() -> (PublicKey, SecretKey) {
        let seed: [u8; 32] = random_array();
        let public = SigningKey::from_bytes(&seed).verifying_key().to_bytes();
        let mut secret = [0u8; 64];
        secret[..32].copy_from_slice(&seed);
        secret[32..].copy_from_slice(&public);
        (PublicKey(public), SecretKey(secret))
    }

    /// The signature of `message`, followed by `message`.
    pub fn sign(message: &[u8], secret: &SecretKey) -> Vec<u8> {
        let seed: [u8; 32] = secret.0[..32].try_into()
                                           .expect("a secret key holds a seed");
        let signature = SigningKey::from_bytes(&seed).sign(message);
        let mut signed = Vec::with_capacity(SIGNATUREBYTES + message.len());
        signed.extend_from_slice(&signature.to_bytes());
        signed.extend_from_slice(message);
        signed
    }

    /// The message in `signed` if its signature is `public`'s.
    pub fn verify(signed: &[u8], public: &PublicKey) -> Result<Vec<u8>, ()> {
        if signed.len() < SIGNATUREBYTES {
            return Err(());
        }
        let (signature, message) = signed.split_at(SIGNATUREBYTES);
        let signature = Signature::from_slice(signature).map_err(|_| ())?;
        let key = VerifyingKey::from_bytes(&public.0).map_err(|_| ())?;
        key.verify_strict(message, &signature).map_err(|_| ())?;
        Ok(message.to_vec())
    }
}

pub mod box_ {
    use crypto_box::{aead::Aead,
                     SalsaBox};

    use super::*;

    bytes_type!(PublicKey, 32, public);
    bytes_type!(SecretKey, 32, secret);
    bytes_type!(Nonce, 24, public);

    pub fn gen_keypair() -> (PublicKey, SecretKey) {
        let secret = crypto_box::SecretKey::from(random_array::<32>());
        (PublicKey(*secret.public_key().as_bytes()), SecretKey(secret.to_bytes()))
    }

    pub fn gen_nonce() -> Nonce { Nonce(random_array()) }

    fn salsa_box(public: &PublicKey, secret: &SecretKey) -> SalsaBox {
        SalsaBox::new(&crypto_box::PublicKey::from(public.0),
                      &crypto_box::SecretKey::from(secret.0))
    }

    /// `message` encrypted from `secret` to `public`.
    pub fn seal(message: &[u8], nonce: &Nonce, public: &PublicKey, secret: &SecretKey) -> Vec<u8> {
        salsa_box(public, secret).encrypt(&nonce.0.into(), message)
                                 .expect("encrypting into memory cannot fail")
    }

    /// The message in `ciphertext`, if it was encrypted from `public` to
    /// `secret`.
    pub fn open(ciphertext: &[u8],
                nonce: &Nonce,
                public: &PublicKey,
                secret: &SecretKey)
                -> Result<Vec<u8>, ()> {
        salsa_box(public, secret).decrypt(&nonce.0.into(), ciphertext)
                                 .map_err(|_| ())
    }
}

pub mod sealedbox {
    use super::box_::{PublicKey,
                      SecretKey};

    /// `message` encrypted to `public` by an anonymous sender.
    pub fn seal(message: &[u8], public: &PublicKey) -> Vec<u8> {
        crypto_box::PublicKey::from(public.0).seal(&mut crypto_box::aead::OsRng, message)
                                             .expect("encrypting into memory cannot fail")
    }

    /// The message in `ciphertext`, if it was sealed to `public`.
    pub fn open(ciphertext: &[u8], public: &PublicKey, secret: &SecretKey) -> Result<Vec<u8>, ()> {
        let secret = crypto_box::SecretKey::from(secret.0);
        if secret.public_key().as_bytes() != &public.0 {
            return Err(());
        }
        secret.unseal(ciphertext).map_err(|_| ())
    }
}

pub mod secretbox {
    use crypto_secretbox::{aead::Aead,
                           KeyInit,
                           XSalsa20Poly1305};

    use super::*;

    bytes_type!(Key, 32, secret);
    bytes_type!(Nonce, 24, public);

    pub fn gen_key() -> Key { Key(random_array()) }

    pub fn gen_nonce() -> Nonce { Nonce(random_array()) }

    pub fn seal(message: &[u8], nonce: &Nonce, key: &Key) -> Vec<u8> {
        XSalsa20Poly1305::new(&key.0.into()).encrypt(&nonce.0.into(), message)
                                            .expect("encrypting into memory cannot fail")
    }

    pub fn open(ciphertext: &[u8], nonce: &Nonce, key: &Key) -> Result<Vec<u8>, ()> {
        XSalsa20Poly1305::new(&key.0.into()).decrypt(&nonce.0.into(), ciphertext)
                                            .map_err(|_| ())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // libsodium 1.0.22's outputs for these inputs, from crypto_sign_seed_keypair,
    // crypto_sign, crypto_box_easy, crypto_box_seal, crypto_secretbox_easy and
    // crypto_generichash. These implementations must match them byte for byte.
    const SIGN_PK: &str = "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";
    const SIGN_SIGNED: &str = "8f9f9ed87618e7ffe70a1a2f1e393f41012d30518f9a9ae3f665a243e05134631\
                               4bb6b6687c42e54012e1a1caad25a1c1d1099d6f98053c1e58ce7005c35460a\
                               646f6a6f";
    const BOX_A_PK: &str = "a4e09292b651c278b9772c569f5fa9bb13d906b46ab68c9df9dc2b4409f8a209";
    const BOX_B_PK: &str = "ce8d3ad1ccb633ec7b70c17814a5c76ecd029685050d344745ba05870e587d59";
    const BOX_EASY: &str = "17910c148d8b6826469f05d556143f2787441560";
    const BOX_SEAL_TO_B: &str = "880e57d3330ba3f6b3171156c1552d07cd801349e5b95a9f6980f88bbf0f160\
                                 579b9186fe2de6e9eb03fb752eec7ca8f5220b147";
    const SECRETBOX_EASY: &str = "a8e93d33bbd6b6a975336f1c4db5d844108d195a";
    const GENERICHASH: &str = "98d6541845944d3d52c53207dca7e4811c95e9fa511872ec91336ee99a0efd9f";

    fn bytes(text: &str) -> Vec<u8> { hex::decode(text).unwrap() }

    fn box_keys(fill: u8) -> (box_::PublicKey, box_::SecretKey) {
        let secret = crypto_box::SecretKey::from([fill; 32]);
        (box_::PublicKey(*secret.public_key().as_bytes()), box_::SecretKey([fill; 32]))
    }

    #[test]
    fn signing_matches_libsodium() {
        let public = ed25519_dalek::SigningKey::from_bytes(&[7; 32]).verifying_key()
                                                                    .to_bytes();
        assert_eq!(public.to_vec(), bytes(SIGN_PK));
        let mut secret = [7u8; 64];
        secret[32..].copy_from_slice(&public);
        let signed = sign::sign(b"dojo", &sign::SecretKey(secret));
        assert_eq!(signed, bytes(SIGN_SIGNED));
        assert_eq!(sign::verify(&signed, &sign::PublicKey(public)).unwrap(),
                   b"dojo");
        let mut tampered = signed;
        tampered[0] ^= 1;
        assert!(sign::verify(&tampered, &sign::PublicKey(public)).is_err());
    }

    #[test]
    fn boxes_match_libsodium() {
        let (a_pk, a_sk) = box_keys(1);
        let (b_pk, b_sk) = box_keys(2);
        assert_eq!(a_pk.0.to_vec(), bytes(BOX_A_PK));
        assert_eq!(b_pk.0.to_vec(), bytes(BOX_B_PK));
        let nonce = box_::Nonce([3; 24]);
        assert_eq!(box_::seal(b"dojo", &nonce, &b_pk, &a_sk), bytes(BOX_EASY));
        assert_eq!(box_::open(&bytes(BOX_EASY), &nonce, &a_pk, &b_sk).unwrap(),
                   b"dojo");
        assert!(box_::open(&bytes(BOX_EASY), &nonce, &b_pk, &b_sk).is_err());
    }

    #[test]
    fn sealed_boxes_open_what_libsodium_sealed() {
        let (a_pk, a_sk) = box_keys(1);
        let (b_pk, b_sk) = box_keys(2);
        assert_eq!(sealedbox::open(&bytes(BOX_SEAL_TO_B), &b_pk, &b_sk).unwrap(),
                   b"dojo");
        assert!(sealedbox::open(&bytes(BOX_SEAL_TO_B), &a_pk, &a_sk).is_err());
        let sealed = sealedbox::seal(b"dojo", &b_pk);
        assert_eq!(sealedbox::open(&sealed, &b_pk, &b_sk).unwrap(), b"dojo");
    }

    #[test]
    fn secretboxes_match_libsodium() {
        let key = secretbox::Key([4; 32]);
        let nonce = secretbox::Nonce([3; 24]);
        assert_eq!(secretbox::seal(b"dojo", &nonce, &key),
                   bytes(SECRETBOX_EASY));
        assert_eq!(secretbox::open(&bytes(SECRETBOX_EASY), &nonce, &key).unwrap(),
                   b"dojo");
        assert!(secretbox::open(&bytes(SECRETBOX_EASY), &nonce, &secretbox::gen_key()).is_err());
    }

    #[test]
    fn hashing_matches_libsodium() {
        assert_eq!(super::super::hash::hash_bytes(b"dojo"), GENERICHASH);
    }

    #[test]
    fn secret_keys_do_not_print() {
        assert_eq!(format!("{:?}", secretbox::gen_key()), "Key(..)");
    }
}
