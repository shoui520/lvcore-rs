use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use aes::Aes128;
use aes::cipher::{BlockDecrypt, KeyInit};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

const LOGOFONT_CIPHER_PASSPHRASE: &[u8] = b"LogoFontCipher";
const BLOCK_SIZE: usize = 16;

pub fn decrypt_logofont_cipher_prefix(data: &[u8], size: usize) -> Result<Vec<u8>> {
    decrypt_logofont_cipher_prefix_with_variant(data, size, LogoFontCipherVariant::Windows)
}

pub fn decrypt_macos_logofont_cipher_prefix(data: &[u8], size: usize) -> Result<Vec<u8>> {
    decrypt_logofont_cipher_prefix_with_variant(data, size, LogoFontCipherVariant::MacOs)
}

fn decrypt_logofont_cipher_prefix_with_variant(
    data: &[u8],
    size: usize,
    variant: LogoFontCipherVariant,
) -> Result<Vec<u8>> {
    if data.len() < BLOCK_SIZE {
        return Ok(Vec::new());
    }
    let size = size.max(BLOCK_SIZE).min(data.len());
    let size = size - (size % BLOCK_SIZE);
    decrypt_logofont_cipher_blocks_with_variant(&data[..size], variant)
}

pub fn decrypt_logofont_cipher_file_to_path(input: &Path, output: &Path) -> Result<()> {
    decrypt_logofont_cipher_file_to_path_with_variant(input, output, LogoFontCipherVariant::Windows)
}

pub fn decrypt_macos_logofont_cipher_file_to_path(input: &Path, output: &Path) -> Result<()> {
    decrypt_logofont_cipher_file_to_path_with_variant(input, output, LogoFontCipherVariant::MacOs)
}

fn decrypt_logofont_cipher_file_to_path_with_variant(
    input: &Path,
    output: &Path,
    variant: LogoFontCipherVariant,
) -> Result<()> {
    let mut infile = File::open(input)?;
    let mut outfile = File::create(output)?;
    let (key, iv) = logofont_cipher_key_iv(variant);
    let cipher = Aes128::new_from_slice(&key)
        .map_err(|_| Error::Driver("invalid LogoFontCipher AES key".to_owned()))?;

    let mut previous_cipher = iv;
    let mut encrypted = vec![0_u8; 1024 * BLOCK_SIZE];
    let mut pending_plain: Option<[u8; BLOCK_SIZE]> = None;

    loop {
        let read = infile.read(&mut encrypted)?;
        if read == 0 {
            break;
        }
        if !read.is_multiple_of(BLOCK_SIZE) {
            return Err(Error::Driver(
                "encrypted payload length is not a multiple of 16 bytes".to_owned(),
            ));
        }
        for chunk in encrypted[..read].chunks_exact(BLOCK_SIZE) {
            let plain = decrypt_cbc_block(&cipher, chunk, &previous_cipher);
            previous_cipher.copy_from_slice(chunk);
            if let Some(previous_plain) = pending_plain.replace(plain) {
                outfile.write_all(&previous_plain)?;
            }
        }
    }

    let Some(last_plain) = pending_plain else {
        return Ok(());
    };
    let unpadded = pkcs7_unpad_or_raw(&last_plain);
    outfile.write_all(unpadded)?;
    Ok(())
}

fn decrypt_logofont_cipher_blocks_with_variant(
    data: &[u8],
    variant: LogoFontCipherVariant,
) -> Result<Vec<u8>> {
    if !data.len().is_multiple_of(BLOCK_SIZE) {
        return Err(Error::Driver(
            "encrypted payload length is not a multiple of 16 bytes".to_owned(),
        ));
    }
    let (key, iv) = logofont_cipher_key_iv(variant);
    let cipher = Aes128::new_from_slice(&key)
        .map_err(|_| Error::Driver("invalid LogoFontCipher AES key".to_owned()))?;
    let mut previous_cipher = iv;
    let mut plaintext = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(BLOCK_SIZE) {
        let plain = decrypt_cbc_block(&cipher, chunk, &previous_cipher);
        previous_cipher.copy_from_slice(chunk);
        plaintext.extend_from_slice(&plain);
    }
    Ok(plaintext)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogoFontCipherVariant {
    Windows,
    MacOs,
}

fn logofont_cipher_key_iv(variant: LogoFontCipherVariant) -> ([u8; BLOCK_SIZE], [u8; BLOCK_SIZE]) {
    let digest = Sha256::digest(LOGOFONT_CIPHER_PASSPHRASE);
    let mut key = [0_u8; BLOCK_SIZE];
    let mut iv = [0_u8; BLOCK_SIZE];
    match variant {
        LogoFontCipherVariant::Windows => {
            key.copy_from_slice(&digest[..BLOCK_SIZE]);
            iv.copy_from_slice(&digest[BLOCK_SIZE..BLOCK_SIZE * 2]);
        }
        LogoFontCipherVariant::MacOs => {
            let hex_digest = hex::encode(digest);
            key.copy_from_slice(&hex_digest.as_bytes()[..BLOCK_SIZE]);
        }
    }
    (key, iv)
}

fn decrypt_cbc_block(
    cipher: &Aes128,
    encrypted_block: &[u8],
    previous_cipher: &[u8; BLOCK_SIZE],
) -> [u8; BLOCK_SIZE] {
    let mut block = aes::Block::clone_from_slice(encrypted_block);
    cipher.decrypt_block(&mut block);
    let mut plain = [0_u8; BLOCK_SIZE];
    for index in 0..BLOCK_SIZE {
        plain[index] = block[index] ^ previous_cipher[index];
    }
    plain
}

fn pkcs7_unpad_or_raw(block: &[u8; BLOCK_SIZE]) -> &[u8] {
    let padding = usize::from(block[BLOCK_SIZE - 1]);
    if padding == 0 || padding > BLOCK_SIZE {
        return block;
    }
    if block[BLOCK_SIZE - padding..]
        .iter()
        .all(|byte| usize::from(*byte) == padding)
    {
        &block[..BLOCK_SIZE - padding]
    } else {
        block
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncrypt;

    #[test]
    fn pkcs7_unpad_falls_back_to_raw_when_padding_is_invalid() {
        let mut block = [b'x'; BLOCK_SIZE];
        block[BLOCK_SIZE - 1] = 2;
        assert_eq!(pkcs7_unpad_or_raw(&block), &block);
    }

    #[test]
    fn pkcs7_unpad_removes_valid_padding() {
        let mut block = [b'a'; BLOCK_SIZE];
        block[14] = 2;
        block[15] = 2;
        assert_eq!(pkcs7_unpad_or_raw(&block), &block[..14]);
    }

    #[test]
    fn macos_logofont_cipher_prefix_uses_observed_key_variant() {
        let encrypted = encrypt_cbc_for_test(
            b"SSEDDATA\x00\x00\x00\x00\x00\x00\x00\x00",
            LogoFontCipherVariant::MacOs,
        );

        let decrypted = decrypt_macos_logofont_cipher_prefix(&encrypted, encrypted.len()).unwrap();

        assert!(decrypted.starts_with(b"SSEDDATA"));
    }

    fn encrypt_cbc_for_test(data: &[u8], variant: LogoFontCipherVariant) -> Vec<u8> {
        let (key, iv) = logofont_cipher_key_iv(variant);
        let cipher = Aes128::new_from_slice(&key).unwrap();
        let mut previous = iv;
        let mut padded = data.to_vec();
        let padding = BLOCK_SIZE - (padded.len() % BLOCK_SIZE);
        padded.extend(std::iter::repeat_n(padding as u8, padding));
        let mut encrypted = Vec::with_capacity(padded.len());
        for chunk in padded.chunks_exact(BLOCK_SIZE) {
            let mut block = [0_u8; BLOCK_SIZE];
            for index in 0..BLOCK_SIZE {
                block[index] = chunk[index] ^ previous[index];
            }
            let mut block = aes::Block::from(block);
            cipher.encrypt_block(&mut block);
            previous.copy_from_slice(&block);
            encrypted.extend_from_slice(&block);
        }
        encrypted
    }
}
