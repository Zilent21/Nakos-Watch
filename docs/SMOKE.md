# Smoke checklist

For a fresh operating-system user with no repository. Run it on a clean account or machine, never on the Mac that holds your live vault. Use generic hosts; the addresses below are placeholders.

1. **Install.** Install the packaged app (later), or copy the `.app`. It opens to "Create your vault".
2. **Create a vault** with a throwaway passphrase. Leave "Remember on this computer" off. Confirm Settings, Vault shows the keychain helper **off**.
3. **Add an xpub.** Portfolio, Add wallet, paste a public extended key (a test-vector key is fine). Nakos never asks for a seed or private key.
4. **Point Nakos at your node.** Node: add a Bitcoin Core RPC connection (`<host>:8332`) and an Electrum server (`<host>:50001` or your TLS port). Use hosts you control.
5. **Sync and plan.** Sync the wallet. In Spend, generate a plan with "Lowest fees" and again with "Highest privacy" and confirm they can differ. Open Inheritance and export both the **simple family letter** and the **advanced packet**.
6. **Second vault: import the letter.** On another clean account create a vault, then Inheritance, "Import a letter or packet", and pick the letter. The wallet appears as watch-only. Open the file: no RPC password, token or passphrase in it; xpubs appear only under "ADVANCED".
7. **Explorer is off by default.** Settings, Public explorer is off. With it off, confirm no outbound explorer traffic (for example with a firewall or network monitor). Turn it on only if you want to test it.
8. **Encrypted backup and restore.** Settings, "Export encrypted backup". On a third clean account choose "Restore from an encrypted backup" on the first-run screen, enter the passphrase, and confirm the wallets are present and the node settings work after unlock. Check a wrong passphrase is refused and a family letter is refused.
9. **Keychain stays off.** Quit and reopen Nakos several times: it asks for the passphrase and shows no OS keychain prompt. The setting stays off unless you turn it on.
