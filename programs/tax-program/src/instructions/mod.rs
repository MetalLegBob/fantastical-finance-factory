//! Tax Program instructions.

pub mod initialize_wsol_intermediary;
pub mod swap_exempt;
pub mod swap_sol_buy;
pub mod swap_sol_sell;

pub use initialize_wsol_intermediary::*;
pub use swap_exempt::*;
pub use swap_sol_buy::*;
pub use swap_sol_sell::*;

pub mod swap_spl_buy;
pub use swap_spl_buy::*;

pub mod swap_spl_sell;
pub use swap_spl_sell::*;

pub mod swap_arb_wallet;
pub use swap_arb_wallet::*;

pub mod withdraw_sweep;
pub use withdraw_sweep::*;

pub mod distribute_swept;
pub use distribute_swept::*;
