//! Post-Quantum Boot Image Signing Tool

use std::fs;
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: {} <sign|verify> <input> [output]", args[0]);
        std::process::exit(1);
    }
    
    match args[1].as_str() {
        "sign" => sign_image(&args[2], &args[3]),
        "verify" => verify_image(&args[2]),
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            std::process::exit(1);
        }
    }
}

fn sign_image(input: &str, output: &str) {
    println!("🔐 Signing {} -> {}", input, output);
    
    // Read input
    let image = fs::read(input).expect("Failed to read input");
    
    // Generate keypair (in production, load from secure storage)
    use pqcrypto_dilithium::dilithium3::*;
    use pqcrypto_traits::sign::{PublicKey, SecretKey};
    
    let (pk, sk) = keypair();
    
    // Sign
    let sig = detached_sign(&image, &sk);
    
    // Build output
    let mut output_data = Vec::new();
    output_data.extend_from_slice(&image);
    output_data.extend_from_slice(sig.as_bytes());
    
    // Add header
    let header = BootSignatureHeader {
        magic: *b"PQSG",
        version: 1,
        sig_len: sig.as_bytes().len() as u32,
        pubkey_len: pk.as_bytes().len() as u32,
    };
    
    output_data.extend_from_slice(unsafe {
        std::slice::from_raw_parts(
            &header as *const _ as *const u8,
            std::mem::size_of::<BootSignatureHeader>()
        )
    });
    
    // Write
    fs::write(output, &output_data).expect("Failed to write output");
    
    // Save public key
    let pubkey_file = format!("{}.pubkey", output);
    fs::write(&pubkey_file, pk.as_bytes()).expect("Failed to write pubkey");
    
    println!("✅ Signed successfully");
    println!("📝 Public key: {}", pubkey_file);
    println!("📊 Original: {} bytes", image.len());
    println!("📊 Signed: {} bytes", output_data.len());
    println!("📊 Overhead: {} bytes", output_data.len() - image.len());
}

fn verify_image(input: &str) {
    println!("🔍 Verifying {}", input);
    
    let image = fs::read(input).expect("Failed to read input");
    
    // Extract header
    let header_size = std::mem::size_of::<BootSignatureHeader>();
    if image.len() < header_size {
        eprintln!("❌ Invalid image: too small");
        std::process::exit(1);
    }
    
    let header_offset = image.len() - header_size;
    let header = unsafe {
        &*(image[header_offset..].as_ptr() as *const BootSignatureHeader)
    };
    
    if &header.magic != b"PQSG" {
        eprintln!("❌ Invalid signature magic");
        std::process::exit(1);
    }
    
    println!("✅ Valid signature format");
    println!("📊 Version: {}", header.version);
    println!("📊 Signature: {} bytes", header.sig_len);
    println!("📊 Public key: {} bytes", header.pubkey_len);
}

#[repr(C)]
struct BootSignatureHeader {
    magic: [u8; 4],
    version: u32,
    sig_len: u32,
    pubkey_len: u32,
}
