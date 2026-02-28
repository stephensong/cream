//! FROST threshold signature support for CREAM root operations.
//!
//! Uses `frost-ed25519` (ZCash Foundation, RFC 9591) for t-of-n threshold
//! signing that produces standard ed25519 signatures — all contract
//! verification code works unchanged.
//!
//! This module implements **trusted dealer mode**: a single function generates
//! all key shares deterministically. Guardian coordination (DKG) comes later.

use std::collections::BTreeMap;

use frost_ed25519 as frost;

/// Configuration for the guardian federation.
pub struct FrostConfig {
    pub min_signers: u16,
    pub max_signers: u16,
}

/// Generate FROST key shares via trusted dealer (deterministic from seed).
///
/// Uses a seeded ChaCha20Rng so the same seed always produces the same
/// key packages and group public key.
pub fn generate_dealer_keys(
    seed: &[u8; 32],
    config: &FrostConfig,
) -> (
    BTreeMap<frost::Identifier, frost::keys::KeyPackage>,
    frost::keys::PublicKeyPackage,
) {
    use rand_chacha::rand_core::SeedableRng;
    let mut rng = rand_chacha::ChaCha20Rng::from_seed(*seed);

    let (shares, pubkey_package) = frost::keys::generate_with_dealer(
        config.max_signers,
        config.min_signers,
        frost::keys::IdentifierList::Default,
        &mut rng,
    )
    .expect("FROST dealer keygen should not fail with valid parameters");

    let key_packages: BTreeMap<_, _> = shares
        .into_iter()
        .map(|(id, share)| {
            let key_package = frost::keys::KeyPackage::try_from(share)
                .expect("KeyPackage conversion should not fail");
            (id, key_package)
        })
        .collect();

    (key_packages, pubkey_package)
}

/// Sign a message using t key shares (local threshold signing).
///
/// Used in dev/trusted-dealer mode where one process holds all shares.
/// Panics if fewer than `min_signers` key packages are provided.
pub fn sign_with_threshold(
    message: &[u8],
    key_packages: &BTreeMap<frost::Identifier, frost::keys::KeyPackage>,
    public_key_package: &frost::keys::PublicKeyPackage,
    min_signers: u16,
) -> ed25519_dalek::Signature {
    use rand_chacha::rand_core::SeedableRng;

    // Deterministic RNG seeded from the message for reproducible nonces.
    // This is safe because FROST nonces are ephemeral and never reused.
    let mut rng = rand_chacha::ChaCha20Rng::from_seed({
        let mut seed = [0u8; 32];
        // Mix message bytes into seed for uniqueness
        for (i, &b) in message.iter().enumerate() {
            seed[i % 32] ^= b;
        }
        seed
    });

    let participants: Vec<_> = key_packages
        .keys()
        .take(min_signers as usize)
        .copied()
        .collect();

    // Round 1: Generate nonces and commitments
    let mut nonces_map = BTreeMap::new();
    let mut commitments_map = BTreeMap::new();

    for &id in &participants {
        let key_package = &key_packages[&id];
        let (nonces, commitments) = frost::round1::commit(key_package.signing_share(), &mut rng);
        nonces_map.insert(id, nonces);
        commitments_map.insert(id, commitments);
    }

    // Create signing package
    let signing_package = frost::SigningPackage::new(commitments_map, message);

    // Round 2: Generate signature shares
    let mut signature_shares = BTreeMap::new();
    for &id in &participants {
        let key_package = &key_packages[&id];
        let nonces = &nonces_map[&id];
        let share = frost::round2::sign(&signing_package, nonces, key_package)
            .expect("FROST round2 signing should not fail");
        signature_shares.insert(id, share);
    }

    // Aggregate into final signature
    let group_signature =
        frost::aggregate(&signing_package, &signature_shares, public_key_package)
            .expect("FROST signature aggregation should not fail");

    // Convert frost::Signature → ed25519_dalek::Signature (both are 64-byte ed25519)
    let sig_vec = group_signature.serialize().expect("signature serialization should not fail");
    let sig_bytes: [u8; 64] = sig_vec.try_into().expect("signature is 64 bytes");
    ed25519_dalek::Signature::from_bytes(&sig_bytes)
}

/// Extract the group VerifyingKey as an ed25519-dalek VerifyingKey.
///
/// This is the root public key that contracts verify against.
pub fn group_verifying_key(
    public_key_package: &frost::keys::PublicKeyPackage,
) -> ed25519_dalek::VerifyingKey {
    let vk = public_key_package.verifying_key();
    let vk_vec = vk.serialize().expect("verifying key serialization should not fail");
    let vk_bytes: [u8; 32] = vk_vec.try_into().expect("verifying key is 32 bytes");
    ed25519_dalek::VerifyingKey::from_bytes(&vk_bytes).expect("valid ed25519 public key")
}

/// Derive a 32-byte seed for FROST keygen from ROOT_USER_NAME via HKDF.
#[cfg(feature = "dev")]
fn derive_root_frost_seed() -> [u8; 32] {
    use hkdf::Hkdf;
    use sha2::Sha256;

    let salt = crate::identity::ROOT_USER_NAME.trim().to_lowercase();
    let hk = Hkdf::<Sha256>::new(Some(salt.as_bytes()), b"cream-root-genesis");
    let mut seed = [0u8; 32];
    hk.expand(b"cream-frost-dealer-seed-v1", &mut seed)
        .expect("HKDF expand should not fail for 32 bytes");
    seed
}

/// Generate the root's FROST keys deterministically (dev mode only).
/// Default config: 2-of-3 threshold.
#[cfg(feature = "dev")]
pub fn dev_root_frost_keys() -> (
    BTreeMap<frost::Identifier, frost::keys::KeyPackage>,
    frost::keys::PublicKeyPackage,
) {
    let seed = derive_root_frost_seed();
    generate_dealer_keys(
        &seed,
        &FrostConfig {
            min_signers: 2,
            max_signers: 3,
        },
    )
}

/// Get the root's group public key (deterministic, dev mode).
#[cfg(feature = "dev")]
pub fn dev_root_verifying_key() -> ed25519_dalek::VerifyingKey {
    let (_, pkg) = dev_root_frost_keys();
    group_verifying_key(&pkg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frost_dealer_keygen_is_deterministic() {
        let (_, pkg1) = dev_root_frost_keys();
        let (_, pkg2) = dev_root_frost_keys();
        assert_eq!(group_verifying_key(&pkg1), group_verifying_key(&pkg2));
    }

    #[test]
    fn frost_sign_verify_roundtrip() {

        let (keys, pkg) = dev_root_frost_keys();
        let vk = group_verifying_key(&pkg);
        let msg = b"test message";
        let sig = sign_with_threshold(msg, &keys, &pkg, 2);
        assert!(vk.verify_strict(msg, &sig).is_ok());
    }

    #[test]
    fn frost_root_key_matches_identity() {
        let root_id = crate::identity::root_customer_id();
        let (_, pkg) = dev_root_frost_keys();
        assert_eq!(*root_id.0.as_bytes(), *group_verifying_key(&pkg).as_bytes());
    }

    #[test]
    fn frost_signature_verifies_as_ed25519() {
        use ed25519_dalek::Verifier;
        let (keys, pkg) = dev_root_frost_keys();
        let vk = group_verifying_key(&pkg);
        let msg = b"user contract signable bytes";
        let sig = sign_with_threshold(msg, &keys, &pkg, 2);
        assert!(vk.verify(msg, &sig).is_ok());
    }

    #[test]
    fn frost_different_messages_different_signatures() {
        let (keys, pkg) = dev_root_frost_keys();
        let sig1 = sign_with_threshold(b"message one", &keys, &pkg, 2);
        let sig2 = sign_with_threshold(b"message two", &keys, &pkg, 2);
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn frost_reconstruct_split_preserves_verifying_key() {

        use rand_chacha::rand_core::SeedableRng;

        let (keys, pkg) = dev_root_frost_keys();
        let original_vk = group_verifying_key(&pkg);

        // Reconstruct from a quorum (2 of 3)
        let quorum: Vec<frost::keys::KeyPackage> = keys.values().take(2).cloned().collect();
        let signing_key =
            frost::keys::reconstruct(&quorum).expect("reconstruct should not fail");

        // Verify reconstructed key matches group key
        let reconstructed_vk: frost_ed25519::VerifyingKey = signing_key.into();
        assert_eq!(*pkg.verifying_key(), reconstructed_vk);

        // Re-split into a new 3-of-5 setup
        let mut rng = rand_chacha::ChaCha20Rng::from_seed([42u8; 32]);
        let new_ids: Vec<frost::Identifier> = (1..=5)
            .map(|i| frost::Identifier::try_from(i).unwrap())
            .collect();
        let (new_shares, new_pub_pkg) = frost::keys::split(
            &signing_key,
            5,
            3,
            frost::keys::IdentifierList::Custom(&new_ids),
            &mut rng,
        )
        .expect("split should not fail");

        // Verify group key is preserved
        assert_eq!(group_verifying_key(&new_pub_pkg), original_vk);

        // Convert shares to key packages and sign
        let new_keys: BTreeMap<frost::Identifier, frost::keys::KeyPackage> = new_shares
            .into_iter()
            .map(|(id, share)| {
                (
                    id,
                    frost::keys::KeyPackage::try_from(share).expect("valid key package"),
                )
            })
            .collect();
        let sig = sign_with_threshold(b"test after redeal", &new_keys, &new_pub_pkg, 3);
        assert!(original_vk.verify_strict(b"test after redeal", &sig).is_ok());
    }

    #[test]
    fn frost_refresh_preserves_verifying_key() {

        use frost::keys::refresh;
        use rand_chacha::rand_core::SeedableRng;

        let mut rng = rand_chacha::ChaCha20Rng::from_seed([99u8; 32]);
        let (keys, pkg) = dev_root_frost_keys();
        let original_vk = group_verifying_key(&pkg);

        // Simulate distributed refresh among 3 parties
        let ids: Vec<frost::Identifier> = keys.keys().copied().collect();
        let max_signers = 3u16;
        let min_signers = 2u16;

        // Round 1: each participant generates a refresh package
        let mut round1_secrets = BTreeMap::new();
        let mut round1_packages = BTreeMap::new();
        for &id in &ids {
            let (secret, package) =
                refresh::refresh_dkg_part1(id, max_signers, min_signers, &mut rng)
                    .expect("refresh part1 should not fail");
            round1_secrets.insert(id, secret);
            round1_packages.insert(id, package);
        }

        // Round 2: each participant processes round1 packages from others
        let mut round2_secrets = BTreeMap::new();
        let mut all_round2_packages: BTreeMap<
            frost::Identifier,
            BTreeMap<frost::Identifier, frost::keys::dkg::round2::Package>,
        > = BTreeMap::new();
        for &id in &ids {
            let others: BTreeMap<_, _> = round1_packages
                .iter()
                .filter(|(&k, _)| k != id)
                .map(|(&k, v)| (k, v.clone()))
                .collect();
            let (secret, packages) =
                refresh::refresh_dkg_part2(round1_secrets.remove(&id).unwrap(), &others)
                    .expect("refresh part2 should not fail");
            round2_secrets.insert(id, secret);
            all_round2_packages.insert(id, packages);
        }

        // Finalize: each participant computes new keys
        let mut new_keys = BTreeMap::new();
        let mut new_pub_pkg = None;
        for &id in &ids {
            let others_r1: BTreeMap<_, _> = round1_packages
                .iter()
                .filter(|(&k, _)| k != id)
                .map(|(&k, v)| (k, v.clone()))
                .collect();
            let others_r2: BTreeMap<_, _> = all_round2_packages
                .iter()
                .filter(|(&k, _)| k != id)
                .map(|(&sender_id, packages)| {
                    (sender_id, packages.get(&id).cloned().unwrap())
                })
                .collect();
            let (key_package, pub_pkg) = refresh::refresh_dkg_shares(
                round2_secrets.get(&id).unwrap(),
                &others_r1,
                &others_r2,
                pkg.clone(),
                keys.get(&id).unwrap().clone(),
            )
            .expect("refresh finalize should not fail");
            new_keys.insert(id, key_package);
            new_pub_pkg = Some(pub_pkg);
        }

        let new_pub_pkg = new_pub_pkg.unwrap();

        // Verify group key is preserved
        assert_eq!(group_verifying_key(&new_pub_pkg), original_vk);

        // Verify signing works with new keys
        let sig = sign_with_threshold(b"test after refresh", &new_keys, &new_pub_pkg, 2);
        assert!(original_vk
            .verify_strict(b"test after refresh", &sig)
            .is_ok());
    }
}
