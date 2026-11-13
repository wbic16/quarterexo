//! GPG interop. qexo NEVER hand-rolls OpenPGP (section 8.7): it shells
//! out to `gpg` with argument vectors — no shell string is ever built
//! from caller input — using a qexo-dedicated keyring beside the
//! state file so fleet keys never mix with a user's personal keyring.
//! The keyring directory is gpg's own machinery; every qexo-owned
//! *record* about keys lives in state.phext, not here.
//!
//! Adapted from the old line's crypto.rs mechanics (the from-scratch
//! brief's named reuse): key generation, fingerprint listing,
//! armored export/import, detached sign/verify, encrypt/decrypt.
//! The one change is the constructor: the old code took the old
//! State type; here the caller passes the state directory (see
//! crate::lattice::state_dir).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct Gpg {
    pub home: String,
}

pub fn gpg_available() -> bool {
    Command::new("gpg")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

impl Gpg {
    /// Open (creating if needed, mode 0700) the qexo keyring under
    /// `<state_dir>/gnupg`.
    pub fn open_in(state_dir: &Path) -> Gpg {
        let dir = state_dir.join("gnupg");
        let _ = std::fs::create_dir_all(&dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        Gpg { home: dir.to_string_lossy().to_string() }
    }

    fn run(&self, args: &[&str], stdin_data: Option<&[u8]>) -> Result<(bool, Vec<u8>, String), String> {
        let mut cmd = Command::new("gpg");
        cmd.arg("--batch")
            .arg("--yes")
            .arg("--homedir")
            .arg(&self.home)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("gpg spawn failed: {}", e))?;
        if let Some(data) = stdin_data {
            let mut stdin = child.stdin.take().ok_or("gpg stdin unavailable")?;
            stdin.write_all(data).map_err(|e| e.to_string())?;
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        Ok((
            out.status.success(),
            out.stdout,
            String::from_utf8_lossy(&out.stderr).to_string(),
        ))
    }

    /// Generate a keypair (ed25519, sign-only for node/persona use).
    /// Returns the fingerprint.
    pub fn generate(&self, name: &str, kind_label: &str) -> Result<String, String> {
        let spec = format!(
            "Key-Type: eddsa\nKey-Curve: ed25519\nKey-Usage: sign\nName-Real: {}\nName-Comment: {}\nExpire-Date: 0\n%no-protection\n%commit\n",
            name.replace('\n', " "),
            kind_label.replace('\n', " ")
        );
        let (ok, _, err) = self.run(&["--gen-key"], Some(spec.as_bytes()))?;
        if !ok {
            return Err(format!("gpg --gen-key failed: {}", err.trim()));
        }
        let fps = self.list_fingerprints()?;
        // the newest key is the last secret key listed for this name
        fps.last().cloned().ok_or_else(|| "no key after generation".to_string())
    }

    pub fn list_fingerprints(&self) -> Result<Vec<String>, String> {
        let (ok, out, err) = self.run(&["--list-secret-keys", "--with-colons"], None)?;
        if !ok {
            return Err(format!("gpg list failed: {}", err.trim()));
        }
        let text = String::from_utf8_lossy(&out);
        let mut fps = Vec::new();
        let mut in_sec = false;
        for line in text.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            match fields.first().copied() {
                Some("sec") => in_sec = true,
                Some("fpr") if in_sec => {
                    if let Some(fp) = fields.get(9) {
                        fps.push(fp.to_string());
                    }
                    in_sec = false;
                }
                _ => {}
            }
        }
        Ok(fps)
    }

    pub fn export_public(&self, fingerprint: &str) -> Result<String, String> {
        let (ok, out, err) = self.run(&["--armor", "--export", fingerprint], None)?;
        if !ok || out.is_empty() {
            return Err(format!("gpg export failed: {}", err.trim()));
        }
        Ok(String::from_utf8_lossy(&out).to_string())
    }

    /// The fingerprint of an armored public key, read WITHOUT
    /// importing it (`--show-keys`): identification is separate from
    /// the act of taking a key into the keyring, so a caller can
    /// learn who a key claims to be before deciding anything.
    pub fn fingerprint_of(&self, armored: &str) -> Result<String, String> {
        let (ok, out, err) = self.run(&["--show-keys", "--with-colons"], Some(armored.as_bytes()))?;
        if !ok {
            return Err(format!("gpg show-keys failed: {}", err.trim()));
        }
        let text = String::from_utf8_lossy(&out);
        for line in text.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.first().copied() == Some("fpr") {
                if let Some(fp) = fields.get(9) {
                    return Ok(fp.to_string());
                }
            }
        }
        Err("no fingerprint in key material".to_string())
    }

    pub fn import_public(&self, armored: &str) -> Result<String, String> {
        let (ok, out, err) = self.run(&["--import"], Some(armored.as_bytes()))?;
        let text = format!("{}{}", String::from_utf8_lossy(&out), err);
        if !ok {
            return Err(format!("gpg import failed: {}", text.trim()));
        }
        Ok(text.trim().to_string())
    }

    /// Detached armored signature over `data`, by `fingerprint`.
    pub fn sign_detached(&self, fingerprint: &str, data: &[u8]) -> Result<String, String> {
        let (ok, out, err) = self.run(
            &["--armor", "--detach-sign", "--local-user", fingerprint],
            Some(data),
        )?;
        if !ok {
            return Err(format!("gpg sign failed: {}", err.trim()));
        }
        Ok(String::from_utf8_lossy(&out).to_string())
    }

    /// Verify a detached armored signature. Returns the signing key's
    /// fingerprint on success. The signature travels via a temp file in
    /// the keyring dir because gpg's verify needs random access to it;
    /// its contents are signature material only.
    pub fn verify_detached(&self, signature_armored: &str, data: &[u8]) -> Result<String, String> {
        let tmp = std::path::Path::new(&self.home).join("verify-tmp.asc");
        std::fs::write(&tmp, signature_armored).map_err(|e| e.to_string())?;
        let (ok, out, err) = self.run(
            &["--status-fd", "1", "--verify", tmp.to_str().unwrap_or("verify-tmp.asc"), "-"],
            Some(data),
        )?;
        let _ = std::fs::remove_file(&tmp);
        let stdout = String::from_utf8_lossy(&out).to_string();
        if ok {
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 && parts[1] == "VALIDSIG" {
                    return Ok(parts[2].to_string());
                }
            }
            return Err("signature valid but signer fingerprint not reported".into());
        }
        Err(format!("signature verification failed: {}", err.trim()))
    }

    /// Encrypt (armored, signed) to recipients — used for keychain
    /// sealing (single holding-node recipient + Will's recovery key as
    /// second recipient when configured: the multi-recipient rule of
    /// section 8A.4).
    pub fn encrypt_to(&self, recipients: &[&str], signer: Option<&str>, data: &[u8]) -> Result<String, String> {
        let mut args: Vec<&str> = vec!["--armor", "--encrypt", "--trust-model", "always"];
        for r in recipients {
            args.push("--recipient");
            args.push(r);
        }
        if let Some(s) = signer {
            args.push("--sign");
            args.push("--local-user");
            args.push(s);
        }
        let (ok, out, err) = self.run(&args, Some(data))?;
        if !ok {
            return Err(format!("gpg encrypt failed: {}", err.trim()));
        }
        Ok(String::from_utf8_lossy(&out).to_string())
    }

    pub fn decrypt(&self, armored: &str) -> Result<Vec<u8>, String> {
        let (ok, out, err) = self.run(&["--decrypt"], Some(armored.as_bytes()))?;
        if !ok {
            return Err(format!("gpg decrypt failed: {}", err.trim()));
        }
        Ok(out)
    }
}
