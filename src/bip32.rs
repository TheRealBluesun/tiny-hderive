#[cfg(feature = "std")]
use base58::FromBase58;
use core::str::FromStr;
use core::{convert::TryInto, ops::Deref};
use hmac::{Hmac, Mac, NewMac};
use k256::{elliptic_curve::sec1::ToEncodedPoint, Scalar};
use k256::{EncodedPoint, PublicKey, SecretKey};
use sha2::Sha512;
#[cfg(feature = "std")]
use std::fmt;

use crate::bip44::{ChildNumber, IntoDerivationPath};
use crate::Error;

#[derive(Clone, PartialEq, Eq)]
pub struct Protected([u8; 32]);

impl<Data: AsRef<[u8]>> From<Data> for Protected {
    fn from(data: Data) -> Protected {
        let mut buf = [0u8; 32];

        buf.copy_from_slice(data.as_ref());

        Protected(buf)
    }
}

impl Deref for Protected {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Drop for Protected {
    fn drop(&mut self) {
        self.0.iter_mut().for_each(|b| *b = 0)
    }
}

#[cfg_attr(feature = "std", derive(Debug, PartialEq, Eq))]
#[derive(Clone)]
pub struct ExtendedPrivKey {
    secret_key: SecretKey,
    chain_code: Protected,
}

impl ExtendedPrivKey {
    /// Attempts to derive an extended private key from a path.
    pub fn derive<Path>(seed: &[u8], path: Path) -> Result<ExtendedPrivKey, Error>
    where
        Path: IntoDerivationPath,
    {
        let mut hmac: Hmac<Sha512> =
            Hmac::new_varkey(b"Bitcoin seed").expect("seed is always correct; qed");
        hmac.update(seed);

        let result = hmac.finalize().into_bytes();
        let (secret_key, chain_code) = result.split_at(32);

        let mut sk = ExtendedPrivKey {
            secret_key: SecretKey::from_bytes(secret_key).map_err(|_| Error::Secp256k1)?,
            chain_code: Protected::from(chain_code),
        };

        for child in path.into()?.as_ref() {
            sk = sk.child(*child)?;
        }

        Ok(sk)
    }

    pub fn secret(&self) -> [u8; 32] {
        let bytes = self.secret_key.to_bytes();
        bytes.as_slice().try_into().unwrap()
    }

    pub fn child(&self, child: ChildNumber) -> Result<ExtendedPrivKey, Error> {
        let mut hmac =
            Hmac::<Sha512>::new_varkey(&self.chain_code).map_err(|_| Error::InvalidChildNumber)?;

        if child.is_normal() {
            // hmac.input(&PublicKey::from_secret_key(&self.secret_key).serialize_compressed()[..]);
            hmac.update(
                self.secret_key
                    .public_key()
                    .to_encoded_point(true)
                    .as_bytes(),
            );
        } else {
            hmac.update(&[0]);
            hmac.update(&self.secret_key.to_bytes()[..]);
        }

        hmac.update(&child.to_bytes());

        let result = hmac.finalize().into_bytes();
        let (secret_key, chain_code) = result.split_at(32);

        let mut secret_key = SecretKey::from_bytes(&secret_key).map_err(|_| Error::Secp256k1)?;
        secret_key = SecretKey::from_bytes(
            (secret_key.secret_scalar().as_ref() + self.secret_key.secret_scalar().as_ref())
                .to_bytes(),
        )
        .map_err(|_| Error::Secp256k1)?;

        Ok(ExtendedPrivKey {
            secret_key,
            chain_code: Protected::from(&chain_code),
        })
        // Ok(ExtendedPrivKey {
        //     secret_key: self.secret_key.clone(),
        //     chain_code: Protected::from(&[0u8; 32]),
        // })
    }
}

#[cfg(feature = "std")]
impl FromStr for ExtendedPrivKey {
    type Err = Error;

    fn from_str(xprv: &str) -> Result<ExtendedPrivKey, Error> {
        let data = xprv
            .from_base58()
            .map_err(|_| Error::InvalidExtendedPrivKey)?;

        if data.len() != 82 {
            return Err(Error::InvalidExtendedPrivKey);
        }

        Ok(ExtendedPrivKey {
            chain_code: Protected::from(&data[13..45]),
            // secret_key: SecretKey::parse_slice(&data[46..78]).map_err(|e| Error::Secp256k1(e))?,
            secret_key: SecretKey::from_bytes(&data[46..78]).map_err(|_| Error::Secp256k1)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bip39::{Language, Mnemonic, Seed};
    use ethsign::SecretKey;

    #[test]
    fn bip39_to_address() {
        let phrase = "panda eyebrow bullet gorilla call smoke muffin taste mesh discover soft ostrich alcohol speed nation flash devote level hobby quick inner drive ghost inside";

        let expected_secret_key = b"\xff\x1e\x68\xeb\x7b\xf2\xf4\x86\x51\xc4\x7e\xf0\x17\x7e\xb8\x15\x85\x73\x22\x25\x7c\x58\x94\xbb\x4c\xfd\x11\x76\xc9\x98\x93\x14";
        let expected_address: &[u8] =
            b"\x63\xF9\xA9\x2D\x8D\x61\xb4\x8a\x9f\xFF\x8d\x58\x08\x04\x25\xA3\x01\x2d\x05\xC8";

        let mnemonic = Mnemonic::from_phrase(phrase, Language::English).unwrap();
        let seed = Seed::new(&mnemonic, "");

        let account = ExtendedPrivKey::derive(seed.as_bytes(), "m/44'/60'/0'/0/0").unwrap();

        assert_eq!(
            expected_secret_key,
            &account.secret(),
            "Secret key is invalid"
        );

        let secret_key = SecretKey::from_raw(&account.secret()).unwrap();
        let public_key = secret_key.public();

        assert_eq!(expected_address, public_key.address(), "Address is invalid");

        // Test child method
        let account = ExtendedPrivKey::derive(seed.as_bytes(), "m/44'/60'/0'/0")
            .unwrap()
            .child(ChildNumber::from_str("0").unwrap())
            .unwrap();

        assert_eq!(
            expected_secret_key,
            &account.secret(),
            "Secret key is invalid"
        );

        let secret_key = SecretKey::from_raw(&account.secret()).unwrap();
        let public_key = secret_key.public();

        assert_eq!(expected_address, public_key.address(), "Address is invalid");
    }
}
