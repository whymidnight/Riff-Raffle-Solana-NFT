pub mod randomness_tools;
pub mod recent_blockhashes;
use anchor_lang::prelude::*;
use anchor_lang::solana_program::sysvar;
use anchor_lang::solana_program::{program::invoke, system_instruction};
use anchor_spl::token::Token;
use anchor_spl::token::{self, Mint, TokenAccount};
use std::str::FromStr;

pub const ENTRANTS_SIZE: u32 = 1000;
pub const TIME_BUFFER: i64 = 1;
pub const FEE_WALLET: &str = "CumSkyxk3mrC6voinTHf3RVj46Az5C65kHpCRwUxmHJ5";
pub const FEE_LAMPORTS: u64 = 12_690_000; // 0.01269 SOL
pub const FEE_LAMPORTS_RAFFLE: u64 = 119_800_000; // 0.1198 SOL

declare_id!("2gPKWB9obxygRWPxEbA1ZHexUsQNHMH7EZ71WER8cc61");

#[program]
pub mod draffle {
    use super::*;

    pub fn create_raffle(
        ctx: Context<CreateRaffle>,
        end_timestamp: i64,
        ticket_price: u64,
        max_entrants: u32,
        name: String,
        image_uri: String,
    ) -> Result<()> {
        let raffle = &mut ctx.accounts.raffle;

        raffle.creator = *ctx.accounts.creator.key;
        raffle.total_prizes = 0;
        raffle.claimed_prizes = 0;
        raffle.randomness = None;
        raffle.end_timestamp = end_timestamp;
        raffle.ticket_price = ticket_price;
        raffle.entrants = ctx.accounts.entrants.key();
        raffle.name = name;
        raffle.image_uri = image_uri;

        let mut entrants = ctx.accounts.entrants.load_init()?;
        if max_entrants > ENTRANTS_SIZE {
            return Err(error!(RaffleError::MaxEntrantsTooLarge));
        }
        entrants.max = max_entrants;

        ctx.accounts.transfer_fee()?;

        Ok(())
    }

    pub fn add_prize(ctx: Context<AddPrize>, prize_index: u32, amount: u64) -> Result<()> {
        let clock = Clock::get()?;
        let raffle = &mut ctx.accounts.raffle;

        if clock.unix_timestamp > raffle.end_timestamp {
            return Err(error!(RaffleError::RaffleEnded));
        }

        if prize_index != raffle.total_prizes {
            return Err(error!(RaffleError::InvalidPrizeIndex));
        }

        if amount == 0 {
            return Err(error!(RaffleError::NoPrize));
        }

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.from.to_account_info(),
                    to: ctx.accounts.prize.to_account_info(),
                    authority: ctx.accounts.creator.to_account_info(),
                },
            ),
            amount,
        )?;

        raffle.total_prizes = raffle
            .total_prizes
            .checked_add(1)
            .ok_or(RaffleError::InvalidCalculation)?;

        Ok(())
    }

    pub fn reclaim_prize(
        ctx: Context<ReclaimPrize>,
        _prize_index: u32,
        _prize_bump: u8,
        amount: u64,
    ) -> Result<()> {
        let raffle = &mut ctx.accounts.raffle;

        let (_, nonce) = Pubkey::find_program_address(
            &[b"raffle".as_ref(), raffle.entrants.as_ref()],
            ctx.program_id,
        );
        let seeds = &[b"raffle".as_ref(), raffle.entrants.as_ref(), &[nonce]];
        let signer_seeds = &[&seeds[..]];
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.prize.to_account_info(),
                    to: ctx.accounts.to.to_account_info(),
                    authority: raffle.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
        )?;

        raffle.total_prizes = raffle
            .total_prizes
            .checked_rem(1)
            .ok_or(RaffleError::InvalidCalculation)?;

        Ok(())
    }

    pub fn buy_tickets(ctx: Context<BuyTickets>, amount: u32) -> Result<()> {
        let clock = Clock::get()?;
        let raffle = &mut ctx.accounts.raffle;
        let mut entrants = ctx.accounts.entrants.load_mut()?;

        if clock.unix_timestamp > raffle.end_timestamp {
            return Err(error!(RaffleError::RaffleEnded));
        }

        for _ in 0..amount {
            entrants.append(ctx.accounts.buyer_token_account.owner)?;
        }

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.buyer_token_account.to_account_info(),
                    to: ctx.accounts.proceeds.to_account_info(),
                    authority: ctx.accounts.buyer_transfer_authority.to_account_info(),
                },
            ),
            raffle
                .ticket_price
                .checked_mul(amount as u64)
                .ok_or(RaffleError::InvalidCalculation)?,
        )?;

        ctx.accounts.transfer_fee()?;

        msg!("Total entrants: {}", { entrants.total });

        Ok(())
    }

    pub fn reveal_winners(ctx: Context<RevealWinners>) -> Result<()> {
        let clock = Clock::get()?;
        let raffle = &mut ctx.accounts.raffle;

        let end_timestamp_with_buffer = raffle
            .end_timestamp
            .checked_add(TIME_BUFFER)
            .ok_or(RaffleError::InvalidCalculation)?;
        // let end_timestamp = raffle.end_timestamp;
        if clock.unix_timestamp < end_timestamp_with_buffer {
            return Err(error!(RaffleError::RaffleStillRunning));
        }

        let randomness =
            recent_blockhashes::last_blockhash_accessor(&ctx.accounts.recent_blockhashes)?;

        match raffle.randomness {
            Some(_) => return Err(error!(RaffleError::WinnersAlreadyDrawn)),
            None => raffle.randomness = Some(randomness),
        }

        Ok(())
    }

    pub fn claim_prize(
        ctx: Context<ClaimPrize>,
        prize_index: u32,
        ticket_index: u32,
    ) -> Result<()> {
        let raffle_account_info = ctx.accounts.raffle.to_account_info();
        let raffle = &mut ctx.accounts.raffle;

        let randomness = match raffle.randomness {
            Some(randomness) => randomness,
            None => return Err(error!(RaffleError::WinnerNotDrawn)),
        };

        let entrants = ctx.accounts.entrants.load()?;

        let winner_rand = randomness_tools::expand(randomness, prize_index);
        let winner_index = winner_rand % entrants.total;

        msg!(
            "Ticket {} attempts claiming prize {} (winner is {})",
            ticket_index,
            prize_index,
            winner_index
        );
        msg!("{} {}", winner_rand, winner_index);
        if ticket_index != winner_index {
            return Err(error!(RaffleError::TicketHasNotWon));
        }

        if ctx.accounts.winner_token_account.owner.key() != entrants.entrants[ticket_index as usize]
        {
            return Err(error!(RaffleError::TokenAccountNotOwnedByWinner));
        }

        if ctx.accounts.prize.amount == 0 {
            return Err(error!(RaffleError::NoPrize));
        }

        let (_, nonce) = Pubkey::find_program_address(
            &[b"raffle".as_ref(), raffle.entrants.as_ref()],
            ctx.program_id,
        );
        let seeds = &[b"raffle".as_ref(), raffle.entrants.as_ref(), &[nonce]];
        let signer_seeds = &[&seeds[..]];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info().clone(),
                token::Transfer {
                    from: ctx.accounts.prize.to_account_info(),
                    to: ctx.accounts.winner_token_account.to_account_info(),
                    authority: raffle_account_info,
                },
                signer_seeds,
            ),
            ctx.accounts.prize.amount,
        )?;

        raffle.claimed_prizes = raffle
            .claimed_prizes
            .checked_add(1)
            .ok_or(RaffleError::InvalidCalculation)?;

        ctx.accounts.transfer_fee()?;

        Ok(())
    }

    pub fn collect_proceeds(ctx: Context<CollectProceeds>) -> Result<()> {
        let raffle = &ctx.accounts.raffle;

        if !raffle.randomness.is_some() {
            return Err(error!(RaffleError::WinnerNotDrawn));
        }

        let (_, nonce) = Pubkey::find_program_address(
            &[b"raffle".as_ref(), raffle.entrants.as_ref()],
            ctx.program_id,
        );
        let seeds = &[b"raffle".as_ref(), raffle.entrants.as_ref(), &[nonce]];
        let signer_seeds = &[&seeds[..]];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.proceeds.to_account_info(),
                    to: ctx.accounts.creator_proceeds.to_account_info(),
                    authority: ctx.accounts.raffle.to_account_info(),
                },
                signer_seeds,
            ),
            ctx.accounts.proceeds.amount,
        )?;

        Ok(())
    }

    pub fn close_entrants(ctx: Context<CloseEntrants>) -> Result<()> {
        let raffle = &ctx.accounts.raffle;
        let entrants = ctx.accounts.entrants.load()?;
        if (raffle.claimed_prizes != raffle.total_prizes) && entrants.total != 0 {
            return Err(error!(RaffleError::UnclaimedPrizes));
        }

        Ok(())
    }
}

#[derive(Accounts)]
pub struct CreateRaffle<'info> {
    #[account(
        init,
        seeds = [b"raffle".as_ref(), entrants.key().as_ref()],
        bump,
        payer = creator,
        space = 8 + 300, // Option serialization workaround
    )]
    pub raffle: Account<'info, Raffle>,
    #[account(zero)]
    pub entrants: AccountLoader<'info, Entrants>,
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(
        init,
        seeds = [raffle.key().as_ref(), b"proceeds"],
        bump,
        payer = creator,
        token::mint = proceeds_mint,
        token::authority = raffle,
    )]
    pub proceeds: Account<'info, TokenAccount>,
    pub proceeds_mint: Account<'info, Mint>,
    /// CHECK:
    #[account(mut, address = Pubkey::from_str(FEE_WALLET).unwrap())]
    pub fee_acc: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
#[instruction(prize_index: u32)]
pub struct AddPrize<'info> {
    #[account(mut, has_one = creator)]
    pub raffle: Account<'info, Raffle>,
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(mut)]
    pub from: Account<'info, TokenAccount>,
    #[account(
        init_if_needed,
        seeds = [raffle.key().as_ref(), b"prize", &prize_index.to_le_bytes()],
        bump,
        payer = creator,
        token::mint = prize_mint,
        token::authority = raffle,
    )]
    pub prize: Account<'info, TokenAccount>,
    pub prize_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct BuyTickets<'info> {
    #[account(has_one = entrants)]
    pub raffle: Account<'info, Raffle>,
    #[account(mut)]
    pub entrants: AccountLoader<'info, Entrants>,
    #[account(
        mut,
        seeds = [raffle.key().as_ref(), b"proceeds"],
        bump,
    )]
    pub proceeds: Account<'info, TokenAccount>,
    // #[account(mut)]
    // pub payer: Signer<'info>,
    #[account(mut)]
    pub buyer_token_account: Account<'info, TokenAccount>,
    #[account(mut, signer)]
    /// CHECK:
    pub buyer_transfer_authority: AccountInfo<'info>,
    /// CHECK:
    #[account(mut, address = Pubkey::from_str(FEE_WALLET).unwrap())]
    pub fee_acc: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RevealWinners<'info> {
    #[account(mut)]
    pub raffle: Account<'info, Raffle>,
    #[account(address = sysvar::recent_blockhashes::ID)]
    /// CHECK:
    pub recent_blockhashes: UncheckedAccount<'info>,
}

#[derive(Accounts)]
#[instruction(prize_index: u32)]
pub struct ClaimPrize<'info> {
    #[account(mut, has_one = entrants)]
    pub raffle: Account<'info, Raffle>,
    pub entrants: AccountLoader<'info, Entrants>,
    #[account(
        mut,
        seeds = [raffle.key().as_ref(), b"prize", &prize_index.to_le_bytes()],
        bump,
    )]
    pub prize: Account<'info, TokenAccount>,
    #[account(mut)]
    pub winner_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK:
    #[account(mut, address = Pubkey::from_str(FEE_WALLET).unwrap())]
    pub fee_acc: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(prize_index: u32, prize_bump: u8)]
pub struct ReclaimPrize<'info> {
    #[account(mut, has_one = creator)]
    pub raffle: Account<'info, Raffle>,
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(mut)]
    pub to: Account<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [raffle.key().as_ref(), b"prize", &prize_index.to_le_bytes()],
        bump = prize_bump
    )]
    pub prize: Account<'info, TokenAccount>,
    pub prize_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct CollectProceeds<'info> {
    #[account(has_one = creator)]
    pub raffle: Account<'info, Raffle>,
    #[account(
        mut,
        seeds = [raffle.key().as_ref(), b"proceeds"],
        bump
    )]
    pub proceeds: Account<'info, TokenAccount>,
    pub creator: Signer<'info>,
    #[account(
        mut,
        constraint = creator_proceeds.owner == creator.key()
    )]
    pub creator_proceeds: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CloseEntrants<'info> {
    #[account(has_one = creator, has_one = entrants)]
    pub raffle: Account<'info, Raffle>,
    #[account(mut, close = creator)]
    pub entrants: AccountLoader<'info, Entrants>,
    pub creator: Signer<'info>,
}

#[account]
#[derive(Default, Debug)]
pub struct Raffle {
    pub creator: Pubkey,
    pub total_prizes: u32,
    pub claimed_prizes: u32,
    pub randomness: Option<[u8; 32]>,
    pub end_timestamp: i64,
    pub ticket_price: u64,
    pub entrants: Pubkey,
    pub name: String,      // ±32 char/byte space alloc
    pub image_uri: String, // ±320 char/byte space alloc
}

impl Raffle {
    pub const LEN: u64 = 8 + 32 + 4 + 4 + 32 + 8 + 8 + 32 + 32 + 320;
}

#[account(zero_copy)]
pub struct Entrants {
    pub total: u32,
    pub max: u32,
    pub entrants: [Pubkey; 1000], // ENTRANTS_SIZE
}

impl Entrants {
    fn append(&mut self, entrant: Pubkey) -> Result<()> {
        if self.total >= self.max {
            return Err(error!(RaffleError::NotEnoughTicketsLeft));
        }
        self.entrants[self.total as usize] = entrant;
        self.total += 1;
        Ok(())
    }
}

impl<'info> CreateRaffle<'info> {
    fn transfer_fee(&self) -> Result<()> {
        invoke(
            &system_instruction::transfer(self.creator.key, self.fee_acc.key, FEE_LAMPORTS_RAFFLE),
            &[
                self.creator.to_account_info(),
                self.fee_acc.clone(),
                self.system_program.to_account_info(),
            ],
        )
        .map_err(Into::into)
    }
}

impl<'info> BuyTickets<'info> {
    fn transfer_fee(&self) -> Result<()> {
        invoke(
            &system_instruction::transfer(
                self.buyer_transfer_authority.key,
                self.fee_acc.key,
                FEE_LAMPORTS,
            ),
            &[
                self.buyer_transfer_authority.to_account_info(),
                self.fee_acc.clone(),
                self.system_program.to_account_info(),
            ],
        )
        .map_err(Into::into)
    }
}

impl<'info> ClaimPrize<'info> {
    fn transfer_fee(&self) -> Result<()> {
        invoke(
            &system_instruction::transfer(self.payer.key, self.fee_acc.key, FEE_LAMPORTS),
            &[
                self.payer.to_account_info(),
                self.fee_acc.clone(),
                self.system_program.to_account_info(),
            ],
        )
        .map_err(Into::into)
    }
}

#[error_code]
pub enum RaffleError {
    #[msg("Max entrants is too large")]
    MaxEntrantsTooLarge,
    #[msg("Raffle has ended")]
    RaffleEnded,
    #[msg("Invalid prize index")]
    InvalidPrizeIndex,
    #[msg("No prize")]
    NoPrize,
    #[msg("Invalid calculation")]
    InvalidCalculation,
    #[msg("Not enough tickets left")]
    NotEnoughTicketsLeft,
    #[msg("Raffle is still running")]
    RaffleStillRunning,
    #[msg("Winner already drawn")]
    WinnersAlreadyDrawn,
    #[msg("Winner not drawn")]
    WinnerNotDrawn,
    #[msg("Invalid revealed data")]
    InvalidRevealedData,
    #[msg("Ticket account not owned by winner")]
    TokenAccountNotOwnedByWinner,
    #[msg("Ticket has not won")]
    TicketHasNotWon,
    #[msg("Unclaimed prizes")]
    UnclaimedPrizes,
    #[msg("Invalid recent blockhashes")]
    InvalidRecentBlockhashes,
}
