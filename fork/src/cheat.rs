// LiteSVM cheats — directly inject SPL Token mint/account state without
// going through token program CPIs. Used to fund attacker/victim keypairs
// for swap testing on cloned mainnet pools (where we can't get real
// memecoins via airdrop).
//
// SPL Token Account layout (165 bytes):
//   mint:           Pubkey       (32)
//   owner:          Pubkey       (32)
//   amount:         u64          (8)
//   delegate:       COption<Pk>  (4 + 32 = 36)
//   state:          u8           (1)   1 = Initialized
//   is_native:      COption<u64> (4 + 8 = 12)
//   delegated_amt:  u64          (8)
//   close_auth:     COption<Pk>  (4 + 32 = 36)

use anyhow::Result;
use litesvm::LiteSVM;
use solana_account::Account;
use solana_pubkey::Pubkey;
use std::str::FromStr;

pub const SPL_TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const TOKEN_ACCOUNT_LEN: usize = 165;
pub const TOKEN_RENT_EXEMPT_LAMPORTS: u64 = 2_039_280;

/// Construct a 165-byte SPL Token Account blob in `Initialized` state.
pub fn build_token_account(mint: &Pubkey, owner: &Pubkey, amount: u64) -> Vec<u8> {
    let mut data = vec![0u8; TOKEN_ACCOUNT_LEN];
    data[0..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(owner.as_ref());
    data[64..72].copy_from_slice(&amount.to_le_bytes());
    // delegate: COption::None = [0,0,0,0] then 32 zero bytes (already 0)
    data[108] = 1; // state = Initialized
    // is_native: COption::None = [0,0,0,0] (already 0)
    // delegated_amount, close_auth: 0 (already 0)
    data
}

/// Inject an SPL Token Account into LiteSVM, fully funded with `amount`
/// tokens of `mint`, owned by `owner`. Returns the address used (caller-supplied).
pub fn fund_token_account(
    svm: &mut LiteSVM,
    address: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
    amount: u64,
) -> Result<()> {
    let token_program = Pubkey::from_str(SPL_TOKEN_PROGRAM).unwrap();
    let acc = Account {
        lamports: TOKEN_RENT_EXEMPT_LAMPORTS,
        data: build_token_account(mint, owner, amount),
        owner: token_program,
        executable: false,
        rent_epoch: 0,
    };
    svm.set_account(*address, acc).map_err(|e| anyhow::anyhow!("set_account: {:?}", e))?;
    Ok(())
}

/// Read SPL Token Account `amount` field (u64 at offset 64).
pub fn read_token_amount(svm: &LiteSVM, address: &Pubkey) -> u64 {
    let acc = svm.get_account(address).expect("token account exists");
    u64::from_le_bytes(acc.data[64..72].try_into().expect("amount field"))
}
