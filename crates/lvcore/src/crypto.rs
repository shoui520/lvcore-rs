use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::LazyLock;

use aes::cipher::{Block, BlockDecrypt, BlockSizeUser, KeyInit, consts::U16};
use aes::{Aes128, Aes256};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

const LOGOFONT_CIPHER_PASSPHRASE: &[u8] = b"LogoFontCipher";
const BLOCK_SIZE: usize = 16;
const STREAM_DECRYPT_BUFFER_SIZE: usize = 64 * 1024 * BLOCK_SIZE;
const ANDROID_DIW_PREFIX: &[u8] = b"LV_";
const ANDROID_DIW_PASSWORD: &str = "resworbncidnatsivogol--emulator";
const ANDROID_DIW_IV: [u8; BLOCK_SIZE] = [
    0x24, 0xc0, 0x15, 0xa3, 0x37, 0xa5, 0x25, 0xae, 0x70, 0xd0, 0x00, 0xcf, 0x0a, 0x41, 0xeb, 0x40,
];
const ANDROID_DIW_SALT: [u8; 20] = [
    0xe4, 0x4d, 0x20, 0x8a, 0x8f, 0xdb, 0x4a, 0xd4, 0x1f, 0x58, 0xdd, 0xe7, 0x61, 0x8b, 0xc8, 0x8f,
    0xe1, 0x34, 0xac, 0x6d,
];
static ANDROID_DIW_AES_KEY: LazyLock<[u8; 32]> = LazyLock::new(derive_android_diw_aes_key_uncached);

pub fn decrypt_logofont_cipher_prefix(data: &[u8], size: usize) -> Result<Vec<u8>> {
    decrypt_logofont_cipher_prefix_with_variant(data, size, LogoFontCipherVariant::Windows)
}

pub fn decrypt_logofont_cipher_bytes(data: &[u8]) -> Result<Vec<u8>> {
    decrypt_logofont_cipher_bytes_with_variant(data, LogoFontCipherVariant::Windows)
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

pub fn decrypt_android_diw_prefix(data: &[u8], size: usize) -> Result<Vec<u8>> {
    if data.len() < BLOCK_SIZE {
        return Ok(Vec::new());
    }
    let take = data.len().min(
        size.saturating_add(ANDROID_DIW_PREFIX.len())
            .div_ceil(BLOCK_SIZE)
            * BLOCK_SIZE,
    );
    let take = take - (take % BLOCK_SIZE);
    if take == 0 {
        return Ok(Vec::new());
    }
    let mut decrypted = decrypt_android_diw_blocks(&data[..take])?;
    if decrypted.starts_with(ANDROID_DIW_PREFIX) {
        decrypted.drain(..ANDROID_DIW_PREFIX.len());
    }
    decrypted.truncate(size);
    Ok(decrypted)
}

pub fn decrypt_android_diw_file_to_path(input: &Path, output: &Path) -> Result<()> {
    let raw_path = output.with_extension("android-diw-raw-tmp");
    if raw_path.exists() {
        fs::remove_file(&raw_path)?;
    }
    if output.exists() {
        fs::remove_file(output)?;
    }
    let result = (|| {
        decrypt_android_diw_file_raw(input, &raw_path)?;
        normalize_android_diw_file(&raw_path, output)
    })();
    let _ = fs::remove_file(&raw_path);
    result
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
    let mut encrypted = vec![0_u8; STREAM_DECRYPT_BUFFER_SIZE];
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
        let plaintext = decrypt_cbc_chunk(&cipher, &encrypted[..read], &mut previous_cipher);
        write_decrypted_chunk_except_final_block(&mut outfile, &mut pending_plain, &plaintext)?;
    }

    let Some(last_plain) = pending_plain else {
        return Ok(());
    };
    let unpadded = pkcs7_unpad_or_raw(&last_plain);
    outfile.write_all(unpadded)?;
    Ok(())
}

fn write_decrypted_chunk_except_final_block(
    outfile: &mut File,
    pending_plain: &mut Option<[u8; BLOCK_SIZE]>,
    plaintext: &[u8],
) -> Result<()> {
    if plaintext.is_empty() {
        return Ok(());
    }
    if !plaintext.len().is_multiple_of(BLOCK_SIZE) {
        return Err(Error::Driver(
            "decrypted payload length is not an AES block multiple".to_owned(),
        ));
    }
    if let Some(previous_plain) = pending_plain.take() {
        outfile.write_all(&previous_plain)?;
    }
    let final_block_start = plaintext.len() - BLOCK_SIZE;
    if final_block_start > 0 {
        outfile.write_all(&plaintext[..final_block_start])?;
    }
    let mut final_block = [0_u8; BLOCK_SIZE];
    final_block.copy_from_slice(&plaintext[final_block_start..]);
    *pending_plain = Some(final_block);
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
    Ok(decrypt_cbc_blocks(&cipher, data, iv))
}

fn decrypt_logofont_cipher_bytes_with_variant(
    data: &[u8],
    variant: LogoFontCipherVariant,
) -> Result<Vec<u8>> {
    let mut plaintext = decrypt_logofont_cipher_blocks_with_variant(data, variant)?;
    if let Some(&last) = plaintext.last() {
        let padding = usize::from(last);
        if padding > 0
            && padding <= BLOCK_SIZE
            && padding <= plaintext.len()
            && plaintext[plaintext.len() - padding..]
                .iter()
                .all(|byte| usize::from(*byte) == padding)
        {
            plaintext.truncate(plaintext.len() - padding);
        }
    }
    Ok(plaintext)
}

fn decrypt_android_diw_file_raw(input: &Path, output: &Path) -> Result<()> {
    let mut infile = File::open(input)?;
    let mut outfile = File::create(output)?;
    let key = derive_android_diw_aes_key();
    let cipher = Aes256::new_from_slice(&key)
        .map_err(|_| Error::Driver("invalid Android HONMON.DIW AES key".to_owned()))?;

    let mut previous_cipher = ANDROID_DIW_IV;
    let mut encrypted = vec![0_u8; STREAM_DECRYPT_BUFFER_SIZE];
    let mut pending_plain: Option<[u8; BLOCK_SIZE]> = None;

    loop {
        let read = infile.read(&mut encrypted)?;
        if read == 0 {
            break;
        }
        if !read.is_multiple_of(BLOCK_SIZE) {
            return Err(Error::Driver(
                "Android HONMON.DIW length is not an AES block multiple".to_owned(),
            ));
        }
        let plaintext = decrypt_cbc_chunk(&cipher, &encrypted[..read], &mut previous_cipher);
        write_decrypted_chunk_except_final_block(&mut outfile, &mut pending_plain, &plaintext)?;
    }

    let Some(last_plain) = pending_plain else {
        return Ok(());
    };
    let unpadded = pkcs7_unpad_or_raw(&last_plain);
    outfile.write_all(unpadded)?;
    Ok(())
}

fn normalize_android_diw_file(input: &Path, output: &Path) -> Result<()> {
    let mut infile = File::open(input)?;
    let mut prefix = [0_u8; 96];
    let read = infile.read(&mut prefix)?;
    let sample = &prefix[..read];
    let prefix_len = usize::from(sample.starts_with(ANDROID_DIW_PREFIX)) * ANDROID_DIW_PREFIX.len();
    let logical = &sample[prefix_len.min(sample.len())..];
    let remove_alignment_pad = logical.starts_with(b"SSEDDATA")
        && logical.len() >= 70
        && u32::from_be_bytes([logical[64], logical[65], logical[66], logical[67]]) == 0
        && u32::from_be_bytes([logical[66], logical[67], logical[68], logical[69]]) > 0;

    let mut outfile = File::create(output)?;
    if remove_alignment_pad {
        copy_file_range(&mut infile, &mut outfile, prefix_len as u64, 64)?;
        infile.seek(SeekFrom::Start((prefix_len + 66) as u64))?;
        std::io::copy(&mut infile, &mut outfile)?;
    } else {
        infile.seek(SeekFrom::Start(prefix_len as u64))?;
        std::io::copy(&mut infile, &mut outfile)?;
    }
    Ok(())
}

fn copy_file_range(infile: &mut File, outfile: &mut File, start: u64, len: u64) -> Result<()> {
    infile.seek(SeekFrom::Start(start))?;
    let mut limited = infile.take(len);
    std::io::copy(&mut limited, outfile)?;
    Ok(())
}

fn decrypt_android_diw_blocks(data: &[u8]) -> Result<Vec<u8>> {
    if !data.len().is_multiple_of(BLOCK_SIZE) {
        return Err(Error::Driver(
            "Android HONMON.DIW length is not an AES block multiple".to_owned(),
        ));
    }
    let key = derive_android_diw_aes_key();
    let cipher = Aes256::new_from_slice(&key)
        .map_err(|_| Error::Driver("invalid Android HONMON.DIW AES key".to_owned()))?;
    Ok(decrypt_cbc_blocks(&cipher, data, ANDROID_DIW_IV))
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

fn decrypt_cbc_blocks<C>(cipher: &C, data: &[u8], iv: [u8; BLOCK_SIZE]) -> Vec<u8>
where
    C: BlockDecrypt + BlockSizeUser<BlockSize = U16>,
{
    let mut previous_cipher = iv;
    decrypt_cbc_chunk(cipher, data, &mut previous_cipher)
}

fn decrypt_cbc_chunk<C>(
    cipher: &C,
    encrypted: &[u8],
    previous_cipher: &mut [u8; BLOCK_SIZE],
) -> Vec<u8>
where
    C: BlockDecrypt + BlockSizeUser<BlockSize = U16>,
{
    let mut blocks: Vec<Block<C>> = encrypted
        .chunks_exact(BLOCK_SIZE)
        .map(Block::<C>::clone_from_slice)
        .collect();
    cipher.decrypt_blocks(&mut blocks);

    let mut plaintext = Vec::with_capacity(encrypted.len());
    for (block, encrypted_block) in blocks.iter().zip(encrypted.chunks_exact(BLOCK_SIZE)) {
        for index in 0..BLOCK_SIZE {
            plaintext.push(block[index] ^ previous_cipher[index]);
        }
        previous_cipher.copy_from_slice(encrypted_block);
    }
    plaintext
}

fn derive_android_diw_aes_key() -> [u8; 32] {
    *ANDROID_DIW_AES_KEY
}

fn derive_android_diw_aes_key_uncached() -> [u8; 32] {
    const U: usize = 20;
    const V: usize = 64;
    const TARGET_LEN: usize = 32;

    let diversifier = [0x01_u8; V];
    let salt = repeat_to_block_multiple(&ANDROID_DIW_SALT, V);
    let password = repeat_to_block_multiple(&pkcs12_password_bytes(ANDROID_DIW_PASSWORD), V);
    let mut blocks = [salt, password].concat();
    let mut out = Vec::with_capacity(40);

    for _ in 0..TARGET_LEN.div_ceil(U) {
        let mut digest = Sha1::digest([diversifier.as_slice(), blocks.as_slice()].concat());
        for _ in 0..1023 {
            digest = Sha1::digest(digest);
        }
        out.extend_from_slice(&digest);
        let adjust = repeat_to_len(&digest, V);
        for offset in (0..blocks.len()).step_by(V) {
            pkcs12_adjust(&mut blocks, offset, &adjust);
        }
    }

    let mut key = [0_u8; 32];
    key.copy_from_slice(&out[..32]);
    key
}

fn pkcs12_password_bytes(password: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(password.len() * 2 + 2);
    for unit in password.encode_utf16() {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out.extend_from_slice(&[0, 0]);
    out
}

fn pkcs12_adjust(blocks: &mut [u8], offset: usize, block: &[u8]) {
    let mut carry =
        u16::from(block[block.len() - 1]) + u16::from(blocks[offset + block.len() - 1]) + 1;
    blocks[offset + block.len() - 1] = carry as u8;
    carry >>= 8;
    for index in (0..block.len() - 1).rev() {
        carry += u16::from(block[index]) + u16::from(blocks[offset + index]);
        blocks[offset + index] = carry as u8;
        carry >>= 8;
    }
}

fn repeat_to_block_multiple(data: &[u8], block_size: usize) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let len = data.len().div_ceil(block_size) * block_size;
    repeat_to_len(data, len)
}

fn repeat_to_len(data: &[u8], len: usize) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        out.extend_from_slice(data);
    }
    out.truncate(len);
    out
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

    #[test]
    fn logofont_cipher_byte_decrypt_strips_full_payload_padding() {
        let encrypted = encrypt_cbc_for_test(
            b"ID3\x03\x00\x00sample mp3 bytes",
            LogoFontCipherVariant::Windows,
        );

        let decrypted = decrypt_logofont_cipher_bytes(&encrypted).unwrap();

        assert_eq!(decrypted, b"ID3\x03\x00\x00sample mp3 bytes");
    }

    #[test]
    fn android_diw_prefix_decrypts_to_sseddata() {
        let encrypted = encrypt_android_diw_for_test(b"LV_SSEDDATA\x00\x00\x00\x00\x00");

        let decrypted = decrypt_android_diw_prefix(&encrypted, 16).unwrap();

        assert!(decrypted.starts_with(b"SSEDDATA"));
    }

    #[test]
    fn android_diw_file_decrypt_strips_wrapper_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("HONMON.DIW");
        let output = dir.path().join("HONMON.DIC");
        std::fs::write(
            &input,
            encrypt_android_diw_for_test(b"LV_SSEDDATA\x00\x00\x00\x00\x00"),
        )
        .unwrap();

        decrypt_android_diw_file_to_path(&input, &output).unwrap();

        let decrypted = std::fs::read(output).unwrap();
        assert!(decrypted.starts_with(b"SSEDDATA"));
        assert!(!decrypted.starts_with(b"LV_"));
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

    fn encrypt_android_diw_for_test(data: &[u8]) -> Vec<u8> {
        let key = derive_android_diw_aes_key();
        let cipher = Aes256::new_from_slice(&key).unwrap();
        let mut previous = ANDROID_DIW_IV;
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
