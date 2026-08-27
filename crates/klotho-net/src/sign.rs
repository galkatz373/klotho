//! Ed25519 join keys and signed [`PlayerIntent`].
//!
//! Signatures authenticate which client sent the packet, not whether a human
//! produced it.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use klotho_ir::PlayerIntent;

use crate::error::NetError;
use crate::packet::{decode_player_intent, encode_player_intent};

/// Ed25519 signature over canonical LE bytes of `value`.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Signed<T> {
    /// 64-byte ed25519 signature.
    pub signature: [u8; 64],
    /// Signed payload.
    pub value: T,
}

/// Join-time keypair. Seeded from OS entropy, not `canon_hash ⊕ tick`.
pub struct Keypair {
    sk: SigningKey,
    vk: VerifyingKey,
}

impl Keypair {
    /// Generate a session identity key.
    pub fn generate() -> Result<Self, NetError> {
        let mut secret = [0u8; 32];
        getrandom::getrandom(&mut secret).map_err(|_| NetError::Keygen)?;
        let sk = SigningKey::from_bytes(&secret);
        let vk = sk.verifying_key();
        Ok(Self { sk, vk })
    }

    /// 32-byte verifying key for Hello.
    #[must_use]
    pub fn verifying_bytes(&self) -> [u8; 32] {
        self.vk.to_bytes()
    }

    pub(crate) fn verifying_key(&self) -> VerifyingKey {
        self.vk
    }

    pub(crate) fn signing_key(&self) -> &SigningKey {
        &self.sk
    }
}

/// Parse a 32-byte verifying key. Wrong length or invalid point is an error.
pub fn verifying_key_from_bytes(bytes: &[u8]) -> Result<VerifyingKey, NetError> {
    if bytes.len() != 32 {
        return Err(NetError::BadKey);
    }
    let mut raw = [0u8; 32];
    raw.copy_from_slice(bytes);
    VerifyingKey::from_bytes(&raw).map_err(|_| NetError::BadKey)
}

/// Sign canonical LE bytes of `intent`.
pub fn sign_intent(kp: &Keypair, intent: &PlayerIntent) -> Result<Signed<PlayerIntent>, NetError> {
    let payload = encode_player_intent(intent)?;
    let sig = kp.signing_key().sign(&payload);
    Ok(Signed {
        signature: sig.to_bytes(),
        value: intent.clone(),
    })
}

/// Verify `msg` (canonical LE intent bytes) against `vk`.
pub fn verify_bytes(vk: &VerifyingKey, signature: &[u8; 64], msg: &[u8]) -> Result<(), NetError> {
    let sig = Signature::from_bytes(signature);
    vk.verify(msg, &sig).map_err(|_| NetError::BadSignature)
}

/// Verify and decode a signed intent. Failures must not be ingested.
pub fn verify_intent(
    vk: &VerifyingKey,
    signed: &Signed<PlayerIntent>,
) -> Result<PlayerIntent, NetError> {
    let payload = encode_player_intent(&signed.value)?;
    verify_bytes(vk, &signed.signature, &payload)?;
    // Re-decode so a struct that cannot round-trip LE is refused.
    decode_player_intent(&payload)
}

#[cfg(test)]
mod tests {
    use klotho_core::{PlayerId, Tick};
    use klotho_ir::{Agency, Analog, IntentTarget, Verb};

    use super::*;
    use crate::packet::encode_player_intent;

    fn look() -> PlayerIntent {
        PlayerIntent {
            player: PlayerId(1),
            at: Tick(0),
            verb: Verb::Look,
            target: IntentTarget::None,
            analog: Analog::default(),
            agency: Agency::none(),
        }
    }

    #[test]
    fn analog_byte_flip_fails_verify() {
        let kp = Keypair::generate().unwrap();
        let mut a = look();
        let mut b = look();
        a.analog.stick_x = 0;
        b.analog.stick_x = 1;
        let ea = encode_player_intent(&a).unwrap();
        let eb = encode_player_intent(&b).unwrap();
        assert_eq!(ea.len(), eb.len());
        let idx = ea
            .iter()
            .zip(eb.iter())
            .position(|(x, y)| x != y)
            .expect("stick_x occupies a LE byte");
        let sig = kp.signing_key().sign(&ea).to_bytes();
        verify_bytes(&kp.verifying_key(), &sig, &ea).unwrap();
        let mut flipped = ea.clone();
        flipped[idx] ^= 1;
        assert_ne!(flipped, ea);
        assert_eq!(
            verify_bytes(&kp.verifying_key(), &sig, &flipped),
            Err(NetError::BadSignature)
        );
    }

    #[test]
    fn truncated_or_off_curve_key_is_error() {
        assert_eq!(verifying_key_from_bytes(&[0u8; 31]), Err(NetError::BadKey));
        assert_eq!(verifying_key_from_bytes(&[]), Err(NetError::BadKey));
        let mut raw = [0u8; 32];
        let mut off_curve = false;
        for i in 0u16..=1024 {
            raw[0] = i as u8;
            raw[1] = (i >> 8) as u8;
            raw[31] = 0x13;
            if verifying_key_from_bytes(&raw).is_err() {
                off_curve = true;
                break;
            }
        }
        assert!(off_curve, "at least one 32-byte encoding must be refused");
    }

    #[test]
    fn sign_round_trip() {
        let kp = Keypair::generate().unwrap();
        let signed = sign_intent(&kp, &look()).unwrap();
        let got = verify_intent(&kp.verifying_key(), &signed).unwrap();
        assert_eq!(got, look());
    }
}
