//! Account derivation from mnemonic phrases.
//!
//! Derives Ethereum accounts using BIP-39 mnemonic + BIP-32 HD derivation,
//! matching the accounts that Anvil generates from its default mnemonic.

use alloy_core::primitives::Bytes;
use alloy_signer_local::{MnemonicBuilder, coins_bip39::English};
use anyhow::{Context, Result};

use crate::AccountInfo;

/// The default mnemonic used by Anvil/Hardhat for generating test accounts.
pub const ANVIL_DEFAULT_MNEMONIC: &str =
    "test test test test test test test test test test test junk";

/// Derive Ethereum accounts from a BIP-39 mnemonic phrase.
///
/// Uses the standard Ethereum HD derivation path `m/44'/60'/0'/0/{index}`
/// to derive `count` accounts. The resulting accounts match those generated
/// by Anvil when started with the same mnemonic.
pub fn derive_accounts_from_mnemonic(mnemonic: &str, count: usize) -> Result<Vec<AccountInfo>> {
    (0..count)
        .map(|index| {
            let wallet = MnemonicBuilder::<English>::default()
                .phrase(mnemonic)
                .index(index as u32)
                .context("Failed to set derivation index")?
                .build()
                .context("Failed to derive wallet from mnemonic")?;

            let address = wallet.address();
            let private_key = wallet.credential().to_bytes();

            Ok(AccountInfo {
                address: Bytes::copy_from_slice(address.as_slice()),
                private_key: Bytes::copy_from_slice(&private_key),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_accounts_from_default_mnemonic() {
        let accounts = derive_accounts_from_mnemonic(ANVIL_DEFAULT_MNEMONIC, 10).unwrap();
        assert_eq!(accounts.len(), 10);

        // Verify first account matches Anvil's known first account
        // Address: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
        let first_address = format!("0x{}", hex::encode(&accounts[0].address));
        assert_eq!(
            first_address.to_lowercase(),
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        );

        // Verify each account has valid address (20 bytes) and private key (32 bytes)
        for account in &accounts {
            assert_eq!(account.address.len(), 20, "Address should be 20 bytes");
            assert_eq!(
                account.private_key.len(),
                32,
                "Private key should be 32 bytes"
            );
        }
    }

    #[test]
    fn test_derive_accounts_deterministic() {
        let accounts1 = derive_accounts_from_mnemonic(ANVIL_DEFAULT_MNEMONIC, 5).unwrap();
        let accounts2 = derive_accounts_from_mnemonic(ANVIL_DEFAULT_MNEMONIC, 5).unwrap();

        for (a, b) in accounts1.iter().zip(accounts2.iter()) {
            assert_eq!(a.address, b.address);
            assert_eq!(a.private_key, b.private_key);
        }
    }
}
