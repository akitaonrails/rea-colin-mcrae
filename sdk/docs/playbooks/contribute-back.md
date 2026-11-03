# Playbook: contribute a finding back

When a launcher's own reverse engineering produced a fix the community does
not have yet (`STANDARDS.md`, "Contribute back").

**Every fix of our own has a draft, from the commit that ships it.** Not
verified yet: draft it anyway with status "needs verification", and post
only after step 1 holds. Give it an id (C1…) and cite the id in its
`docs/RESEARCH.md` row; a fix that can't be shared says `no draft:
<reason>` there. `sdk/scripts/launcher-check` (rule `own-fix-draft`) fails
on a row decided **Our own**, a byte patch or a hook/preload library with
no draft.

1. **Qualify it.** Verified in the real game (`docs/RUNTIME_NOTES.md`), tied
   to an exact executable (SHA-256 and version), and not already on the
   game's PCGamingWiki page or in an upstream project.
2. **Draft it** in the SDK's `docs/CONTRIBUTIONS.md` (or the launcher's own
   `docs/CONTRIBUTIONS.md`): number, target (wiki page and section, upstream
   repo, ProtonDB, WineHQ AppDB), text ready to paste, evidence, status.
   - PCGamingWiki: follow the page's fix-box style; give file offsets with
     original → new bytes, or INI/registry values; cite the launcher's
     public repository; never attach or link game files. Describe DRM/disc
     check removal only in general terms (wiki policy).
   - Upstream projects: an issue (or PR) in their style, with evidence.
   - WineHQ AppDB accepts results for plain Wine only, not for wrappers.
3. **Ask the owner.** Posting is public and under the owner's name: post
   only a draft the owner approved by number ("post #3").
4. **Post.**
   - GitHub: `gh issue create` / `gh pr create` once approved.
   - PCGamingWiki: the site is behind a Cloudflare browser challenge; do not
     try to get around it. Until staff allow API use, hand the owner the
     exact wikitext and page link to paste. If API access is granted, log
     in with the owner's bot password read from
     `~/.config/rea/pcgamingwiki.txt` at runtime.
   - ProtonDB / AppDB: web forms only; give the owner the text.
5. **Record** the outcome (link, date) in the draft's status.

## Credentials

Never print, log, commit, pass on a command line, or hand to another agent
any credential. Scripts read credential files themselves and print only
success or failure.
