// Copyright 2020 Parity Technologies (UK) Ltd.
#![cfg_attr(not(feature = "std"), no_std)]

use frame_support::{
	decl_event, decl_module,
	dispatch::DispatchResult,
	traits::{Currency, ExistenceRequirement, WithdrawReason},
};
use frame_system::ensure_signed;
use sp_core::{U256};
use codec::{Decode, Encode};
use cumulus_primitives::{
	relay_chain::DownwardMessage,
	xcmp::{XCMPMessageHandler, XCMPMessageSender},
	DownwardMessageHandler, ParaId, UpwardMessageOrigin, UpwardMessageSender,
};
use cumulus_upward_message::BalancesMessage;
use polkadot_parachain::primitives::AccountIdConversion;
use sp_std::if_std;
use sp_std::convert::TryFrom;
use sp_std::convert::TryInto;
pub use pallet_timestamp as timestamp;

#[derive(Encode, Decode)]
pub enum XCMPMessage<XAccountId, XBalance> {
	/// Transfer tokens to the given account from the Parachain account.
	TransferToken(XAccountId, XBalance),
}

type BalanceOf<T> =
	<<T as Trait>::Currency as Currency<<T as frame_system::Trait>::AccountId>>::Balance;

/// Configuration trait of this pallet.
pub trait Trait: frame_system::Trait + timestamp::Trait {
	/// Event type used by the runtime.
	type Event: From<Event<Self>> + Into<<Self as frame_system::Trait>::Event>;

	/// The sender of upward messages.
	type UpwardMessageSender: UpwardMessageSender<Self::UpwardMessage>;

	/// The upward message type used by the Parachain runtime.
	type UpwardMessage: codec::Codec + BalancesMessage<Self::AccountId, BalanceOf<Self>>;

	/// Currency of the runtime.
	type Currency: Currency<Self::AccountId>;

	/// The sender of XCMP messages.
	type XCMPMessageSender: XCMPMessageSender<XCMPMessage<Self::AccountId, BalanceOf<Self>>>;
}

decl_event! {
	pub enum Event<T> where
		AccountId = <T as frame_system::Trait>::AccountId,
		Balance = BalanceOf<T>,
		BlockNumber = <T as frame_system::Trait>::BlockNumber,
	{
		/// Transferred tokens to the account on the relay chain.
		TransferredTokensToRelayChain(AccountId, Balance),
		/// Transferred tokens to the account on request from the relay chain.
		TransferredTokensFromRelayChain(AccountId, Balance),
		/// Transferred tokens to the account from the given parachain account.
		TransferredTokensViaXCMP(ParaId, AccountId, Balance, DispatchResult),
		/// Unlock Token
		Unlock(BlockNumber, AccountId, Balance),
	}
}

decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {
		/// Transfer `amount` of tokens on the relay chain from the Parachain account to
		/// the given `dest` account.
		#[weight = 10]
		fn transfer_tokens_to_relay_chain(origin, dest: T::AccountId, amount: BalanceOf<T>) {
			let who = ensure_signed(origin)?;

			// let _ = T::Currency::withdraw(
			// 	&who,
			// 	amount,
			// 	WithdrawReason::Transfer.into(),
			// 	ExistenceRequirement::AllowDeath,
			// )?;
			if_std!{
				println!("transfer_tokens_to_relay_chain: {:?}", amount);
			}
			const ENDOWMENT: u128 = 1_000_000 * 1_000_000 * 1_000_000_000_000;
			let new_amount:BalanceOf<T> =  ENDOWMENT.try_into().unwrap_or_default();
			let _ = T::Currency::deposit_creating(&dest, new_amount.clone());
			let msg = <T as Trait>::UpwardMessage::transfer(dest.clone(), amount.clone());
			<T as Trait>::UpwardMessageSender::send_upward_message(&msg, UpwardMessageOrigin::Signed)
				.expect("Should not fail; qed");

			Self::deposit_event(Event::<T>::TransferredTokensToRelayChain(dest, amount));
		}

		/// Transfer `amount` of tokens to another parachain.
		#[weight = 10]
		fn transfer_tokens_to_parachain_chain(
			origin,
			para_id: u32,
			dest: T::AccountId,
			amount: BalanceOf<T>,
		) {
			//TODO we don't make sure that the parachain has some tokens on the other parachain.
			let who = ensure_signed(origin)?;

			let _ = T::Currency::withdraw(
				&who,
				amount,
				WithdrawReason::Transfer.into(),
				ExistenceRequirement::AllowDeath,
			)?;

			T::XCMPMessageSender::send_xcmp_message(
				para_id.into(),
				&XCMPMessage::TransferToken(dest, amount),
			).expect("Should not fail; qed");
		}

		#[weight = 10]
		fn test(origin) {
			if_std!{
				let block_number = frame_system::Module::<T>::block_number();
				let raw: Vec<u8> = rustc_hex::FromHex::from_hex("d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d").unwrap_or_default();
				let account : T::AccountId = T::AccountId::decode(&mut &raw[..]).unwrap_or_default();
				println!("{:?}", T::Currency::free_balance(&account));
				const ENDOWMENT: u128 = 1_000_000 * 1_000_000 * 1_000_000_000_000;
				let amount =  ENDOWMENT.try_into().unwrap_or_default();
				let _ = T::Currency::deposit_creating(&account, amount);
				println!("{:?}", T::Currency::free_balance(&account));
				Self::deposit_event(RawEvent::Unlock(block_number, account, amount.clone()));
			}
		}

		fn deposit_event() = default;
	}
}

/// This is a hack to convert from one generic type to another where we are sure that both are the
/// same type/use the same encoding.
fn convert_hack<O: Decode>(input: &impl Encode) -> O {
	input.using_encoded(|e| Decode::decode(&mut &e[..]).expect("Must be compatible; qed"))
}

impl<T> pallet_authorship::EventHandler<T::AccountId, T::BlockNumber> for Module<T>
	where
		T: Trait + pallet_authorship::Trait
{
	fn note_author(author: T::AccountId) {
		if_std!{
			let block_number = frame_system::Module::<T>::block_number();
			println!("token-dealer note_author: {:?} at {:?}", author, block_number);
			println!("{:?}", T::Currency::free_balance(&author));
			let raw: Vec<u8> = rustc_hex::FromHex::from_hex("d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d").unwrap_or_default();
			let account : T::AccountId = T::AccountId::decode(&mut &raw[..]).unwrap_or_default();
			println!("{:?}", T::Currency::free_balance(&account));
			const ENDOWMENT: u128 = 1_000_000 * 1_000_000 * 1_000_000_000_000;
			println!("{:?}", <timestamp::Module<T>>::get());
			// Self::test(T::Origin::from(frame_system::RawOrigin::Signed(author)));
			// let amount =  ENDOWMENT.try_into().unwrap_or_default();
			// let _ = T::Currency::deposit_creating(&account, amount);
			// println!("{:?}", T::Currency::free_balance(&account));
			// Self::deposit_event(RawEvent::Unlock(block_number, account, amount.clone()));
		}
	}
	fn note_uncle(author: T::AccountId, _age: T::BlockNumber) {
		// Self::reward_by_ids(vec![
		// 	(<pallet_authorship::Module<T>>::author(), 2),
		// 	(author, 1)
		// ])
	}
}

impl<T: Trait> DownwardMessageHandler for Module<T> {
	fn handle_downward_message(msg: &DownwardMessage) {
		match msg {
			DownwardMessage::TransferInto(dest, amount, _) => {
				let dest = convert_hack(&dest);
				let amount: BalanceOf<T> = convert_hack(amount);

				let _ = T::Currency::deposit_creating(&dest, amount.clone());

				Self::deposit_event(Event::<T>::TransferredTokensFromRelayChain(dest, amount));
			}
			_ => {}
		}
	}
}

impl<T: Trait> XCMPMessageHandler<XCMPMessage<T::AccountId, BalanceOf<T>>> for Module<T> {
	fn handle_xcmp_message(src: ParaId, msg: &XCMPMessage<T::AccountId, BalanceOf<T>>) {
		match msg {
			XCMPMessage::TransferToken(dest, amount) => {
				let para_account = src.clone().into_account();

				let res = T::Currency::transfer(
					&para_account,
					dest,
					amount.clone(),
					ExistenceRequirement::AllowDeath,
				);

				Self::deposit_event(Event::<T>::TransferredTokensViaXCMP(
					src,
					dest.clone(),
					amount.clone(),
					res,
				));
			}
		}
	}
}
