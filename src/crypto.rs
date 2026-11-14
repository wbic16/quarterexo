//! OpenPGP mechanics — in-process since v1.6.0 (DESIGN §8.7).
//!
//! DESIGN §8.7's rule stands: qexo never hand-rolls OpenPGP.
//! Through v1.5.0 the mechanics were delegated to the system
//! `gpg` binary, spawned per operation — workable on a server,
//! impossible where a node lives inside an Android app and no
//! subprocess is ours to spawn. v1.6.0 keeps the rule and
//! changes the delegate: OpenPGP is the `pgp` crate (rpgp), a
//! published implementation of RFC 4880/9580 linked into the
//! binary — the same move v1.3.0 made for phext, when the
//! hand-rolled parser gave way to libphext-rs. Interop with
//! GnuPG is a gate of this slice, not an assumption: this
//! code's signatures verify under system gpg, GnuPG's
//! signatures verify here, and keys cross in both directions
//! with their fingerprints intact.
//!
//! The key store lives in the keyring directory beside the
//! state file — `<state dir>/gnupg`, mode 0700 on Unix, the
//! name unchanged so doctor and the fleet's layout carry
//! over — under `qexo-keys/`: the node's own secret key as
//! `own.secret.asc` (armored, mode 0600) and every public key
//! (the node's own and its peers') as `<FINGERPRINT>.asc`
//! (armored). qexo keys are Ed25519 signing keys without a
//! passphrase, exactly as the gpg era generated them.
//!
//! NO STRANDED KEYS: a keyring directory from the gpg era —
//! `pubring.kbx` plus `private-keys-v1.d/*.key` — is migrated
//! the first time a key operation opens it. The conversion is
//! format-only: GnuPG stores an unprotected key's secret as
//! an S-expression apart from its public keyblock; the
//! migration rebuilds the standard secret-key packet around
//! GnuPG's own public-key bytes and that secret, so the
//! fingerprint — a hash over those same public bytes — is
//! the node's identity, unchanged. (KBX also interleaves
//! GnuPG-local trust packets in each keyblock; those are
//! bookkeeping, not key material, and are dropped on the
//! way in, as gpg itself drops them on export — left in,
//! they stop a parser binding the key's user id and
//! self-signature.) The GnuPG files are never
//! modified, so a rolled-back pre-1.6 binary still finds its
//! keyring exactly as it left it. If a legacy key cannot be
//! converted (a passphrase-protected key, an unknown curve),
//! the error names the reason and generation REFUSES to birth
//! a replacement over it: a node never quietly re-keys.

use std::path::{Path, PathBuf};

use pgp::composed::{
    ArmorOptions, Deserializable, DetachedSignature, KeyType, Message, MessageBuilder,
    SecretKeyParamsBuilder, SignedPublicKey, SignedSecretKey,
};
use pgp::crypto::{hash::HashAlgorithm, sym::SymmetricKeyAlgorithm};
use pgp::ser::Serialize;
use pgp::types::{Fingerprint, KeyDetails, Password};

/// The node's keyring: a handle on the keyring directory,
/// nothing more — key material is read from the store on each
/// operation, so a `Gpg` never holds stale keys.
pub struct Gpg {
    pub home: String,
}

fn es(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// qexo prints fingerprints the way GnuPG does: uppercase hex,
/// no spaces.
fn fp_string(fp: &Fingerprint) -> String {
    fp.as_bytes().iter().map(|b| format!("{:02X}", b)).collect()
}

fn parse_public(armored: &str) -> Result<SignedPublicKey, String> {
    SignedPublicKey::from_armor_single(armored.as_bytes())
        .map(|(k, _)| k)
        .map_err(|e| format!("unparsable public key material: {}", e))
}

fn parse_secret(armored: &str) -> Result<SignedSecretKey, String> {
    SignedSecretKey::from_armor_single(armored.as_bytes())
        .map(|(k, _)| k)
        .map_err(|e| format!("unparsable secret key material: {}", e))
}

fn parse_public_binary(bytes: &[u8]) -> Result<SignedPublicKey, String> {
    let mut iter = SignedPublicKey::from_bytes_many(bytes).map_err(es)?;
    match iter.next() {
        Some(Ok(k)) => Ok(k),
        Some(Err(e)) => Err(format!("unparsable public key material: {}", e)),
        None => Err("no public key in the given bytes".to_string()),
    }
}

fn parse_secret_binary(bytes: &[u8]) -> Result<SignedSecretKey, String> {
    let mut iter = SignedSecretKey::from_bytes_many(bytes).map_err(es)?;
    match iter.next() {
        Some(Ok(k)) => Ok(k),
        Some(Err(e)) => Err(format!("unparsable secret key material: {}", e)),
        None => Err("no secret key in the given bytes".to_string()),
    }
}

fn first_uid(key: &SignedPublicKey) -> Result<String, String> {
    key.details
        .users
        .first()
        .map(|u| String::from_utf8_lossy(u.id.id()).into_owned())
        .ok_or_else(|| "the key carries no user id".to_string())
}

impl Gpg {
    /// Root the keyring at a state DIRECTORY (the state file's
    /// parent): the gnupg home is created beside the state
    /// file if needed, mode 0700 on Unix. Used by every caller
    /// that already knows where the state lives.
    pub fn open_in(state_dir: &Path) -> Gpg {
        let home = state_dir.join("gnupg");
        let _ = std::fs::create_dir_all(&home);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700));
        }
        Gpg {
            home: home.display().to_string(),
        }
    }

    fn home_path(&self) -> &Path {
        Path::new(&self.home)
    }

    fn store_dir(&self) -> PathBuf {
        self.home_path().join("qexo-keys")
    }

    fn own_secret_path(&self) -> PathBuf {
        self.store_dir().join("own.secret.asc")
    }

    fn public_path(&self, fingerprint: &str) -> PathBuf {
        self.store_dir().join(format!("{}.asc", fingerprint))
    }

    /// Make the internal store ready: create it, and if the
    /// directory still holds only GnuPG-era material, migrate
    /// it (see the module note). Idempotent; a store that
    /// already has the node's secret key is left as it is,
    /// apart from topping up public keys the legacy keyring
    /// holds that the store lacks.
    fn ensure_store(&self) -> Result<(), String> {
        let store = self.store_dir();
        std::fs::create_dir_all(&store)
            .map_err(|e| format!("cannot create key store {}: {}", store.display(), e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o700));
        }
        if self.own_secret_path().exists() {
            self.import_missing_publics();
            return Ok(());
        }
        if !self.legacy_secret_files().is_empty() {
            self.migrate_legacy()?;
        }
        Ok(())
    }

    fn import_missing_publics(&self) {
        if let Ok(legacy) = self.legacy_publics() {
            for (key, _) in legacy {
                let fp = fp_string(&key.fingerprint());
                let dest = self.public_path(&fp);
                if !dest.exists() {
                    if let Ok(armor) = key.to_armored_string(ArmorOptions::default()) {
                        let _ = std::fs::write(&dest, armor);
                    }
                }
            }
        }
    }

    fn store_public(&self, key: &SignedPublicKey) -> Result<String, String> {
        let fp = fp_string(&key.fingerprint());
        let armor = key
            .to_armored_string(ArmorOptions::default())
            .map_err(es)?;
        std::fs::write(self.public_path(&fp), armor)
            .map_err(|e| format!("cannot store public key {}: {}", fp, e))?;
        Ok(fp)
    }

    fn load_own_secret(&self) -> Result<SignedSecretKey, String> {
        self.ensure_store()?;
        let path = self.own_secret_path();
        let text = std::fs::read_to_string(&path)
            .map_err(|_| "this node has no secret key in its keyring".to_string())?;
        parse_secret(&text)
    }

    fn load_public(&self, fingerprint: &str) -> Result<SignedPublicKey, String> {
        self.ensure_store()?;
        let path = self.public_path(fingerprint);
        if let Ok(text) = std::fs::read_to_string(&path) {
            return parse_public(&text);
        }
        // The node's own public half derives from its secret.
        if let Ok(secret) = self.load_own_secret() {
            let public = secret.to_public_key();
            if fp_string(&public.fingerprint()) == fingerprint {
                return Ok(public);
            }
        }
        Err(format!("key {} is not in the keyring", fingerprint))
    }

    /// Every public key the keyring holds: the node's own
    /// (derived from its secret when present) plus the stored
    /// peer keys, deduplicated by fingerprint.
    fn all_publics(&self) -> Vec<SignedPublicKey> {
        let mut out: Vec<SignedPublicKey> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut push = |key: SignedPublicKey| {
            let fp = fp_string(&key.fingerprint());
            if !seen.contains(&fp) {
                seen.push(fp);
                out.push(key);
            }
        };
        if let Ok(secret) = self.load_own_secret() {
            push(secret.to_public_key());
        }
        if let Ok(entries) = std::fs::read_dir(self.store_dir()) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".asc") && name != "own.secret.asc" {
                    if let Ok(text) = std::fs::read_to_string(entry.path()) {
                        if let Ok(key) = parse_public(&text) {
                            push(key);
                        }
                    }
                }
            }
        }
        out
    }

    /// Generate this node's key: an Ed25519 signing key whose
    /// user id is `name (kind_label)` — the same shape the gpg
    /// era's batch spec produced. Unprotected, as qexo node
    /// keys have always been. Idempotent: a node that already
    /// has a secret key keeps it — and a node whose GnuPG-era
    /// key cannot be migrated gets the migration error here,
    /// never a silent replacement key.
    pub fn generate(&self, name: &str, kind_label: &str) -> Result<String, String> {
        self.ensure_store()?;
        if let Ok(secret) = self.load_own_secret() {
            return Ok(fp_string(&secret.to_public_key().fingerprint()));
        }
        if !self.legacy_secret_files().is_empty() {
            return Err(
                "a GnuPG-era secret key is present but could not be migrated; refusing to generate a replacement over it"
                    .to_string(),
            );
        }
        let mut params = SecretKeyParamsBuilder::default();
        params
            .key_type(KeyType::Ed25519Legacy)
            .can_sign(true)
            .can_certify(false)
            .primary_user_id(format!("{} ({})", name, kind_label));
        let params = params.build().map_err(es)?;
        let secret = params.generate(rand::thread_rng()).map_err(es)?;
        let public = secret.to_public_key();
        let fp = fp_string(&public.fingerprint());
        let armor = secret
            .to_armored_string(ArmorOptions::default())
            .map_err(es)?;
        // Write via a sibling temp file + rename, so an
        // interrupted first birth never leaves a torn secret
        // key the store would mistake for a whole one.
        let dest = self.own_secret_path();
        let tmp = self.store_dir().join("own.secret.asc.tmp");
        std::fs::write(&tmp, armor)
            .map_err(|e| format!("cannot write the node secret key: {}", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &dest)
            .map_err(|e| format!("cannot install the node secret key: {}", e))?;
        self.store_public(&public)?;
        Ok(fp)
    }

    /// The fingerprints of the secret keys this keyring
    /// holds — in practice the node's own, exactly one.
    pub fn list_fingerprints(&self) -> Result<Vec<String>, String> {
        match self.load_own_secret() {
            Ok(secret) => Ok(vec![fp_string(&secret.to_public_key().fingerprint())]),
            Err(e) => {
                if self.own_secret_path().exists() || !self.legacy_secret_files().is_empty() {
                    Err(e)
                } else {
                    Ok(Vec::new())
                }
            }
        }
    }

    /// The armored public key for a fingerprint in the
    /// keyring.
    pub fn export_public(&self, fingerprint: &str) -> Result<String, String> {
        let key = self.load_public(fingerprint)?;
        key.to_armored_string(ArmorOptions::default()).map_err(es)
    }

    /// The fingerprint of armored public key material,
    /// without importing it.
    pub fn fingerprint_of(&self, armored: &str) -> Result<String, String> {
        let key = parse_public(armored)?;
        Ok(fp_string(&key.fingerprint()))
    }

    /// The user id (name) carried by armored public key
    /// material — what gpg's colon listing used to report and
    /// `register` derives a peer's host from.
    pub fn name_of(&self, armored: &str) -> Result<String, String> {
        let key = parse_public(armored)?;
        first_uid(&key)
    }

    /// Import an armored public key into the keyring.
    pub fn import_public(&self, armored: &str) -> Result<String, String> {
        let key = parse_public(armored)?;
        self.ensure_store()?;
        let fp = self.store_public(&key)?;
        Ok(format!("imported {}", fp))
    }

    /// A detached, armored signature over `data` by the named
    /// key (which must be this node's own — the only secret
    /// key a keyring holds).
    pub fn sign_detached(&self, fingerprint: &str, data: &[u8]) -> Result<String, String> {
        let secret = self.load_own_secret()?;
        let own = fp_string(&secret.to_public_key().fingerprint());
        if own != fingerprint {
            return Err(format!(
                "secret key {} is not in the keyring (this node holds {})",
                fingerprint, own
            ));
        }
        let sig = DetachedSignature::sign_binary_data(
            rand::thread_rng(),
            &secret.primary_key,
            &Password::empty(),
            HashAlgorithm::Sha256,
            data,
        )
        .map_err(es)?;
        sig.to_armored_string(ArmorOptions::default()).map_err(es)
    }

    /// Verify a detached armored signature over `data`
    /// against every public key the keyring holds; on success
    /// returns the signing key's fingerprint — the VALIDSIG
    /// fingerprint the update path's trust check compares.
    pub fn verify_detached(&self, armored_sig: &str, data: &[u8]) -> Result<String, String> {
        let (sig, _) = DetachedSignature::from_armor_single(armored_sig.as_bytes())
            .map_err(|e| format!("unparsable signature: {}", e))?;
        let issuers: Vec<String> = sig
            .signature
            .issuer_fingerprint()
            .iter()
            .map(|f| fp_string(f))
            .collect();
        for key in self.all_publics() {
            let fp = fp_string(&key.fingerprint());
            if !issuers.is_empty() && !issuers.contains(&fp) {
                continue;
            }
            if sig.verify(&key, data).is_ok() {
                return Ok(fp);
            }
        }
        Err("signature does not verify under any key in the keyring".to_string())
    }

    /// Encrypt `data` to every recipient fingerprint (all must
    /// be in the keyring), optionally signing with this node's
    /// own secret key first. The signed-then-sealed persona
    /// keychain exchange (DESIGN §4.7).
    pub fn encrypt_to(
        &self,
        recipients: &[String],
        signer: Option<&str>,
        data: &[u8],
    ) -> Result<String, String> {
        let mut builder = MessageBuilder::from_bytes("", data.to_vec());
        let secret = match signer {
            Some(fp) => {
                let secret = self.load_own_secret()?;
                let own = fp_string(&secret.to_public_key().fingerprint());
                if own != fp {
                    return Err(format!("signing key {} is not in the keyring", fp));
                }
                Some(secret)
            }
            None => None,
        };
        if let Some(secret) = &secret {
            builder.sign(&secret.primary_key, Password::empty(), HashAlgorithm::Sha256);
        }
        // seipd_v1 transitions the builder into its
        // encryption-capable state.
        let mut builder = builder.seipd_v1(&mut rand::thread_rng(), SymmetricKeyAlgorithm::AES256);
        for fp in recipients {
            let key = self.load_public(fp)?;
            builder
                .encrypt_to_key(&mut rand::thread_rng(), &key)
                .map_err(es)?;
        }
        builder
            .to_armored_string(&mut rand::thread_rng(), ArmorOptions::default())
            .map_err(es)
    }

    /// Decrypt an armored message with this node's own key.
    pub fn decrypt(&self, armored: &str) -> Result<Vec<u8>, String> {
        use std::io::Read;
        let (message, _) = Message::from_armor(armored.as_bytes())
            .map_err(|e| format!("unparsable message: {}", e))?;
        let secret = self.load_own_secret()?;
        let mut decrypted = message.decrypt(&Password::empty(), &secret).map_err(es)?;
        let mut out = Vec::new();
        decrypted
            .read_to_end(&mut out)
            .map_err(|e| format!("cannot read the decrypted message: {}", e))?;
        Ok(out)
    }

    // ---- the GnuPG-era migration ---------------------------

    /// The legacy secret key files GnuPG's agent wrote:
    /// `private-keys-v1.d/*.key`, sorted for determinism.
    fn legacy_secret_files(&self) -> Vec<PathBuf> {
        let dir = self.home_path().join("private-keys-v1.d");
        let mut files: Vec<PathBuf> = match std::fs::read_dir(&dir) {
            Ok(entries) => entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "key").unwrap_or(false))
                .collect(),
            Err(_) => Vec::new(),
        };
        files.sort();
        files
    }

    /// The legacy public keyring: every transferable public
    /// key in `pubring.kbx` (or, on older homes, `pubring.gpg`),
    /// each with the raw keyblock bytes GnuPG wrote — the
    /// secret-key rebuild splices around those very bytes.
    fn legacy_publics(&self) -> Result<Vec<(SignedPublicKey, Vec<u8>)>, String> {
        let kbx = self.home_path().join("pubring.kbx");
        if kbx.exists() {
            let bytes = std::fs::read(&kbx)
                .map_err(|e| format!("cannot read {}: {}", kbx.display(), e))?;
            let mut out = Vec::new();
            for block in kbx_keyblocks(&bytes) {
                // KBX stores GnuPG's local trust packets
                // interleaved in the keyblock; they are not
                // part of a transferable key (gpg itself strips
                // them on export), and a parser meeting one
                // stops binding the key's remaining packets —
                // the user id and self-signature would be lost.
                let block = strip_trust_packets(&block);
                if let Ok(key) = parse_public_binary(&block) {
                    out.push((key, block));
                }
            }
            return Ok(out);
        }
        let ring = self.home_path().join("pubring.gpg");
        if ring.exists() {
            let bytes = std::fs::read(&ring)
                .map_err(|e| format!("cannot read {}: {}", ring.display(), e))?;
            let mut out = Vec::new();
            let iter = SignedPublicKey::from_bytes_many(&bytes[..]).map_err(es)?;
            for item in iter {
                let key = item.map_err(es)?;
                let raw = key.to_bytes().map_err(es)?;
                out.push((key, raw));
            }
            return Ok(out);
        }
        Ok(Vec::new())
    }

    /// Convert a GnuPG-era keyring in place — additively: the
    /// internal store is written, the GnuPG files are only
    /// ever read. See the module note for the contract.
    fn migrate_legacy(&self) -> Result<(), String> {
        let publics = self.legacy_publics()?;
        // Peer publics carry over wholesale.
        for (key, _) in &publics {
            self.store_public(key)?;
        }
        // The node's own secret: match each legacy secret to
        // the public keyblock whose point it holds, rebuild
        // the secret key around that keyblock, and accept the
        // result only when its fingerprint is the keyblock's.
        let mut last_err = String::new();
        for file in self.legacy_secret_files() {
            let bytes = std::fs::read(&file)
                .map_err(|e| format!("cannot read {}: {}", file.display(), e))?;
            let legacy = match parse_agent_key(&bytes) {
                Ok(l) => l,
                Err(e) => {
                    last_err = e;
                    continue;
                }
            };
            let d = match &legacy.d {
                Some(d) => d.clone(),
                None => {
                    return Err(format!(
                        "the GnuPG-era key in {} is passphrase-protected; internal migration carries unprotected keys only — the original keyring is untouched",
                        file.display()
                    ));
                }
            };
            let matched = publics.iter().find(|(_, block)| {
                public_point(block)
                    .map(|p| point_matches(&p, &legacy.q))
                    .unwrap_or(false)
            });
            let (key, block) = match matched {
                Some(m) => m,
                None => {
                    last_err = format!(
                        "no public keyblock in the GnuPG keyring matches the secret in {}",
                        file.display()
                    );
                    continue;
                }
            };
            let secret_bytes = build_secret_keyblock(block, &d)?;
            let secret = parse_secret_binary(&secret_bytes)?;
            let rebuilt_fp = fp_string(&secret.to_public_key().fingerprint());
            let want_fp = fp_string(&key.fingerprint());
            if rebuilt_fp != want_fp {
                return Err(format!(
                    "migration rebuilt fingerprint {} but the node key is {} — refusing the conversion; the original keyring is untouched",
                    rebuilt_fp, want_fp
                ));
            }
            let armor = secret
                .to_armored_string(ArmorOptions::default())
                .map_err(es)?;
            let dest = self.own_secret_path();
            let tmp = self.store_dir().join("own.secret.asc.tmp");
            std::fs::write(&tmp, armor)
                .map_err(|e| format!("cannot write the migrated secret key: {}", e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            }
            std::fs::rename(&tmp, &dest)
                .map_err(|e| format!("cannot install the migrated secret key: {}", e))?;
            return Ok(());
        }
        Err(if last_err.is_empty() {
            "no migratable secret key found in the GnuPG-era keyring".to_string()
        } else {
            last_err
        })
    }
}

/// One keyblock's worth of public keys out of a KBX file.
/// KBX is a sequence of length-prefixed blobs; a type-2 blob
/// carries, at offset 8, the offset of its keyblock within
/// the blob and, at offset 12, the keyblock's length (the
/// blob's trailing 20 bytes are its SHA-1 checksum).
fn kbx_keyblocks(bytes: &[u8]) -> Vec<Vec<u8>> {
    fn u32be(b: &[u8], at: usize) -> Option<u32> {
        b.get(at..at + 4)
            .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= bytes.len() {
        let len = match u32be(bytes, pos) {
            Some(v) => v as usize,
            None => break,
        };
        if len < 16 || pos + len > bytes.len() {
            break;
        }
        let blob = &bytes[pos..pos + len];
        if blob[4] == 2 {
            if let (Some(off), Some(klen)) = (u32be(blob, 8), u32be(blob, 12)) {
                let (off, klen) = (off as usize, klen as usize);
                if off >= 16 && off + klen <= len {
                    out.push(blob[off..off + klen].to_vec());
                }
            }
        }
        pos += len;
    }
    out
}

/// Remove trust packets (tag 12) from a keyblock, keeping
/// every other packet byte-for-byte. Trust packets are
/// GnuPG-local bookkeeping, never part of a transferable
/// key; KBX embeds them, keyrings and exports do not.
fn strip_trust_packets(block: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(block.len());
    let mut pos = 0usize;
    while pos < block.len() {
        match split_first_packet(&block[pos..]) {
            Some((tag, _, total)) if total > 0 && pos + total <= block.len() => {
                if tag != 12 {
                    out.extend_from_slice(&block[pos..pos + total]);
                }
                pos += total;
            }
            _ => {
                // Unparseable tail: keep it rather than
                // silently truncate a key.
                out.extend_from_slice(&block[pos..]);
                break;
            }
        }
    }
    out
}

/// Split the first packet of a keyblock: (tag, body start,
/// total packet length), across old- and new-format headers.
fn split_first_packet(bytes: &[u8]) -> Option<(u8, usize, usize)> {
    let b0 = *bytes.first()?;
    if b0 & 0x80 == 0 {
        return None;
    }
    if b0 & 0x40 != 0 {
        // New format.
        let tag = b0 & 0x3f;
        let o1 = *bytes.get(1)? as usize;
        let (len, hdr) = match o1 {
            0..=191 => (o1, 2),
            192..=223 => {
                let o2 = *bytes.get(2)? as usize;
                (((o1 - 192) << 8) + o2 + 192, 3)
            }
            255 => {
                let v = bytes.get(2..6)?;
                (u32::from_be_bytes([v[0], v[1], v[2], v[3]]) as usize, 6)
            }
            _ => return None, // partial body lengths never occur in key material
        };
        Some((tag, hdr, hdr + len))
    } else {
        // Old format.
        let tag = (b0 >> 2) & 0x0f;
        let (len, hdr) = match b0 & 0x03 {
            0 => (*bytes.get(1)? as usize, 2),
            1 => {
                let v = bytes.get(1..3)?;
                (u16::from_be_bytes([v[0], v[1]]) as usize, 3)
            }
            2 => {
                let v = bytes.get(1..5)?;
                (u32::from_be_bytes([v[0], v[1], v[2], v[3]]) as usize, 5)
            }
            _ => return None,
        };
        Some((tag, hdr, hdr + len))
    }
}

/// The public point MPI payload of a keyblock's primary key
/// packet (for an Ed25519 key: 0x40 followed by the 32-byte
/// compressed point). Reads the packet body directly:
/// version, time, algorithm, curve OID, then the MPI.
fn public_point(keyblock: &[u8]) -> Option<Vec<u8>> {
    let (tag, body_at, total) = split_first_packet(keyblock)?;
    if tag != 6 {
        return None;
    }
    let body = keyblock.get(body_at..total)?;
    // version(1) time(4) algorithm(1) then the curve OID as a
    // length-prefixed byte string.
    let oid_len = *body.get(6)? as usize;
    let mpi_at = 7 + oid_len;
    let bitlen = body.get(mpi_at..mpi_at + 2)?;
    let bits = u16::from_be_bytes([bitlen[0], bitlen[1]]) as usize;
    let nbytes = (bits + 7) / 8;
    body.get(mpi_at + 2..mpi_at + 2 + nbytes).map(|s| s.to_vec())
}

/// Compressed-point equality, tolerating the 0x40 prefix
/// being present on one side only.
fn point_matches(a: &[u8], b: &[u8]) -> bool {
    fn norm(p: &[u8]) -> &[u8] {
        if p.len() == 33 && p[0] == 0x40 {
            &p[1..]
        } else {
            p
        }
    }
    norm(a) == norm(b)
}

/// An MPI: two-byte bit length, then the value with leading
/// zero bytes stripped.
fn mpi_encode(value: &[u8]) -> Vec<u8> {
    let mut v = value;
    while v.len() > 1 && v[0] == 0 {
        v = &v[1..];
    }
    if v.is_empty() || (v.len() == 1 && v[0] == 0) {
        return vec![0, 0];
    }
    let lead = v[0].leading_zeros() as usize;
    let bits = (v.len() * 8 - lead) as u16;
    let mut out = Vec::with_capacity(2 + v.len());
    out.extend_from_slice(&bits.to_be_bytes());
    out.extend_from_slice(v);
    out
}

/// A new-format packet header for `tag` over `body_len`
/// bytes.
fn packet_header(tag: u8, body_len: usize) -> Vec<u8> {
    let mut out = vec![0xc0 | tag];
    if body_len < 192 {
        out.push(body_len as u8);
    } else if body_len <= 8383 {
        let v = body_len - 192;
        out.push(192 + (v >> 8) as u8);
        out.push((v & 0xff) as u8);
    } else {
        out.push(255);
        out.extend_from_slice(&(body_len as u32).to_be_bytes());
    }
    out
}

/// Rebuild a transferable SECRET key from GnuPG's public
/// keyblock and the secret scalar its agent stored apart.
/// The secret-key packet is GnuPG's public-key packet body —
/// version, creation time, algorithm, curve, point, all
/// byte-for-byte — followed by an unprotected S2K octet, the
/// secret MPI, and its checksum; the rest of the keyblock
/// (user id, self-signatures) follows verbatim, so the
/// rebuilt key's fingerprint and self-signatures are the
/// originals by construction.
fn build_secret_keyblock(pub_block: &[u8], secret: &[u8]) -> Result<Vec<u8>, String> {
    let (tag, body_at, total) = split_first_packet(pub_block)
        .ok_or_else(|| "the legacy public keyblock does not start with a packet".to_string())?;
    if tag != 6 {
        return Err(format!(
            "the legacy public keyblock starts with packet tag {}, not a public key",
            tag
        ));
    }
    let body = &pub_block[body_at..total];
    let mpi = mpi_encode(secret);
    let checksum: u16 = mpi
        .iter()
        .fold(0u32, |acc, b| acc + *b as u32)
        .rem_euclid(65536) as u16;
    let mut secret_body = Vec::with_capacity(body.len() + 1 + mpi.len() + 2);
    secret_body.extend_from_slice(body);
    secret_body.push(0); // S2K usage octet: no protection
    secret_body.extend_from_slice(&mpi);
    secret_body.extend_from_slice(&checksum.to_be_bytes());
    let mut out = packet_header(5, secret_body.len());
    out.extend_from_slice(&secret_body);
    out.extend_from_slice(&pub_block[total..]);
    Ok(out)
}

/// A GnuPG agent key file's payload, reduced to what the
/// migration needs.
struct LegacyEcc {
    q: Vec<u8>,
    d: Option<Vec<u8>>,
}

/// Parse a `private-keys-v1.d/*.key` file: GnuPG's advanced
/// S-expression — `(private-key (ecc (curve Ed25519) (flags
/// eddsa) (q #…#) (d #…#)))`, possibly preceded by header
/// lines (`Created:`, `Key:`). A `(protected …)` form instead
/// of `(d …)` yields `d: None` — the caller refuses it.
fn parse_agent_key(bytes: &[u8]) -> Result<LegacyEcc, String> {
    let start = bytes
        .iter()
        .position(|b| *b == b'(')
        .ok_or_else(|| "no S-expression in the agent key file".to_string())?;
    let mut parser = SexpParser {
        bytes: &bytes[start..],
        pos: 0,
    };
    let root = parser.parse()?;
    let items = match &root {
        Sexp::List(items) => items,
        _ => return Err("the agent key file is not a list".to_string()),
    };
    if items.first().and_then(|s| s.as_atom()) != Some(b"private-key".as_slice()) {
        return Err("the agent key file is not a private-key".to_string());
    }
    let ecc = items
        .iter()
        .filter_map(|s| match s {
            Sexp::List(l) => Some(l),
            _ => None,
        })
        .find(|l| l.first().and_then(|s| s.as_atom()) == Some(b"ecc".as_slice()))
        .ok_or_else(|| "the agent key is not an ecc key".to_string())?;
    let mut curve_ok = false;
    let mut q: Option<Vec<u8>> = None;
    let mut d: Option<Vec<u8>> = None;
    let mut protected = false;
    for item in ecc.iter().skip(1) {
        let list = match item {
            Sexp::List(l) => l,
            _ => continue,
        };
        let head = list.first().and_then(|s| s.as_atom()).unwrap_or(b"");
        let value = list.get(1).and_then(|s| s.as_atom()).unwrap_or(b"");
        match head {
            b"curve" => curve_ok = value == b"Ed25519",
            b"q" => q = Some(value.to_vec()),
            b"d" => d = Some(value.to_vec()),
            b"protected" | b"protected-at" | b"shadowed-private-key" => protected = true,
            _ => {}
        }
    }
    if !curve_ok {
        return Err(
            "the GnuPG-era key is not an Ed25519 key; internal migration carries Ed25519 only"
                .to_string(),
        );
    }
    if protected {
        return Ok(LegacyEcc {
            q: q.ok_or_else(|| "protected agent key carries no public point".to_string())?,
            d: None,
        });
    }
    Ok(LegacyEcc {
        q: q.ok_or_else(|| "the agent key carries no public point".to_string())?,
        d: Some(d.ok_or_else(|| "the agent key carries no secret".to_string())?),
    })
}

enum Sexp {
    Atom(Vec<u8>),
    List(Vec<Sexp>),
}

impl Sexp {
    fn as_atom(&self) -> Option<&[u8]> {
        match self {
            Sexp::Atom(a) => Some(a),
            _ => None,
        }
    }
}

/// A small reader for GnuPG's S-expression surface: canonical
/// length-prefixed atoms, `#hex#` atoms (which GnuPG folds
/// across lines), and bare atoms.
struct SexpParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> SexpParser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn parse(&mut self) -> Result<Sexp, String> {
        self.skip_ws();
        match self.bytes.get(self.pos) {
            Some(b'(') => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_ws();
                    match self.bytes.get(self.pos) {
                        Some(b')') => {
                            self.pos += 1;
                            return Ok(Sexp::List(items));
                        }
                        Some(_) => items.push(self.parse()?),
                        None => return Err("unbalanced S-expression".to_string()),
                    }
                }
            }
            Some(_) => self.parse_atom().map(Sexp::Atom),
            None => Err("empty S-expression".to_string()),
        }
    }

    fn parse_atom(&mut self) -> Result<Vec<u8>, String> {
        if self.bytes.get(self.pos) == Some(&b'#') {
            // #hex# — GnuPG folds long hex across lines, so
            // whitespace inside is not data.
            let rel_end = self.bytes[self.pos + 1..]
                .iter()
                .position(|b| *b == b'#')
                .ok_or_else(|| "unterminated #hex# atom".to_string())?;
            let end = self.pos + 1 + rel_end;
            let raw: Vec<u8> = self.bytes[self.pos + 1..end]
                .iter()
                .copied()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
            self.pos = end + 1;
            if raw.len() % 2 != 0 {
                return Err("odd-length #hex# atom".to_string());
            }
            let mut out = Vec::with_capacity(raw.len() / 2);
            for pair in raw.chunks(2) {
                let hi = (pair[0] as char)
                    .to_digit(16)
                    .ok_or_else(|| "bad hex in #hex# atom".to_string())?;
                let lo = (pair[1] as char)
                    .to_digit(16)
                    .ok_or_else(|| "bad hex in #hex# atom".to_string())?;
                out.push((hi * 16 + lo) as u8);
            }
            return Ok(out);
        }
        // A digit run followed by ':' is a canonical
        // length-prefixed atom.
        let save = self.pos;
        let mut n = 0usize;
        let mut digits = 0usize;
        while let Some(b) = self.bytes.get(self.pos) {
            if b.is_ascii_digit() {
                n = n * 10 + (*b - b'0') as usize;
                self.pos += 1;
                digits += 1;
            } else {
                break;
            }
        }
        if digits > 0 && self.bytes.get(self.pos) == Some(&b':') {
            self.pos += 1;
            let from = self.pos;
            if from + n > self.bytes.len() {
                return Err("length-prefixed atom overruns the file".to_string());
            }
            self.pos += n;
            return Ok(self.bytes[from..from + n].to_vec());
        }
        self.pos = save;
        // A bare atom: everything up to whitespace or a paren.
        let from = self.pos;
        while let Some(b) = self.bytes.get(self.pos) {
            if b.is_ascii_whitespace() || *b == b'(' || *b == b')' {
                break;
            }
            self.pos += 1;
        }
        if self.pos == from {
            return Err("empty atom in S-expression".to_string());
        }
        Ok(self.bytes[from..self.pos].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("qexo-crypto-test-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp home");
        dir
    }

    #[test]
    fn internal_roundtrip_sign_verify_and_names() {
        let dir = temp_home("roundtrip");
        let gpg = Gpg::open_in(&dir);
        let fp = gpg.generate("qexo node test", "qexo node key").expect("generate");
        assert_eq!(fp.len(), 40);
        assert_eq!(gpg.list_fingerprints().expect("list"), vec![fp.clone()]);
        let public = gpg.export_public(&fp).expect("export");
        assert!(public.contains("BEGIN PGP PUBLIC KEY BLOCK"));
        assert_eq!(gpg.fingerprint_of(&public).expect("fp of"), fp);
        assert_eq!(
            gpg.name_of(&public).expect("name of"),
            "qexo node test (qexo node key)"
        );
        let data = b"a manifest-shaped scroll";
        let sig = gpg.sign_detached(&fp, data).expect("sign");
        assert!(sig.contains("BEGIN PGP SIGNATURE"));
        assert_eq!(gpg.verify_detached(&sig, data).expect("verify"), fp);
        let mut tampered = data.to_vec();
        tampered[0] ^= 1;
        assert!(gpg.verify_detached(&sig, &tampered).is_err());
        // a stranger's keyring verifies nothing of ours
        let other_dir = temp_home("roundtrip-other");
        let other = Gpg::open_in(&other_dir);
        other
            .generate("qexo node stranger", "qexo node key")
            .expect("generate stranger");
        assert!(other.verify_detached(&sig, data).is_err());
        // and generation is idempotent: the same key comes back
        assert_eq!(
            gpg.generate("qexo node test", "qexo node key").expect("regen"),
            fp
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other_dir);
    }

    #[test]
    fn legacy_gnupg_home_migrates_with_fingerprint_intact() {
        // A real GnuPG-era keyring, generated by system gpg
        // with qexo's own batch spec (throwaway fixture key —
        // see tests/fixtures/legacy-gnupg/README).
        const FIXTURE_FP: &str = "C67424B8836BBA88E1F5B8B824570838961846C6";
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy-gnupg");
        let dir = temp_home("migrate");
        let home = dir.join("gnupg");
        std::fs::create_dir_all(home.join("private-keys-v1.d")).expect("fixture dirs");
        std::fs::copy(fixture.join("pubring.kbx"), home.join("pubring.kbx")).expect("fixture kbx");
        let mut key_file = None;
        for entry in std::fs::read_dir(fixture.join("private-keys-v1.d")).expect("fixture keys") {
            let entry = entry.expect("fixture key entry");
            key_file = Some(entry.file_name());
            std::fs::copy(entry.path(), home.join("private-keys-v1.d").join(entry.file_name()))
                .expect("fixture key");
        }
        let key_file = key_file.expect("fixture has a key");
        let legacy_before =
            std::fs::read(home.join("private-keys-v1.d").join(&key_file)).expect("legacy read");
        let kbx_before = std::fs::read(home.join("pubring.kbx")).expect("kbx read");

        // open_in roots the keyring at dir/gnupg — the fixture
        // home assembled above.
        let gpg = Gpg::open_in(&dir);
        let fps = gpg.list_fingerprints().expect("migrated list");
        assert_eq!(fps, vec![FIXTURE_FP.to_string()]);
        assert!(
            gpg.own_secret_path().exists(),
            "migration wrote the internal store"
        );
        let data = b"release manifest, fixture edition";
        let sig = gpg.sign_detached(FIXTURE_FP, data).expect("migrated sign");
        assert_eq!(
            gpg.verify_detached(&sig, data).expect("migrated verify"),
            FIXTURE_FP
        );
        let public = gpg.export_public(FIXTURE_FP).expect("migrated export");
        assert_eq!(gpg.fingerprint_of(&public).expect("fp"), FIXTURE_FP);
        // the user id must survive the migration: KBX interleaves
        // GnuPG trust packets in the keyblock, and losing the uid
        // makes the exported key unimportable by gpg (this
        // assertion is the regression test for exactly that)
        assert_eq!(
            gpg.name_of(&public).expect("migrated name"),
            "qexo node fixture (qexo node key)"
        );
        // the GnuPG originals are byte-identical afterwards
        assert_eq!(
            std::fs::read(home.join("private-keys-v1.d").join(&key_file)).expect("legacy re-read"),
            legacy_before
        );
        assert_eq!(
            std::fs::read(home.join("pubring.kbx")).expect("kbx re-read"),
            kbx_before
        );
        // and the store survives a second open untouched
        assert_eq!(
            gpg.list_fingerprints().expect("second list"),
            vec![FIXTURE_FP.to_string()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn protected_legacy_key_is_refused_not_replaced() {
        let dir = temp_home("protected");
        let home = dir.join("gnupg");
        std::fs::create_dir_all(home.join("private-keys-v1.d")).expect("dirs");
        std::fs::write(
            home.join("private-keys-v1.d").join("DEADBEEF.key"),
            "(private-key (ecc (curve Ed25519)(flags eddsa)(q #404142#)(protected openpgp-native ())))",
        )
        .expect("write protected fixture");
        let gpg = Gpg::open_in(&dir);
        let err = gpg
            .generate("qexo node test", "qexo node key")
            .expect_err("protected key must refuse generation");
        assert!(err.contains("protected"), "{}", err);
        assert!(!gpg.own_secret_path().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
