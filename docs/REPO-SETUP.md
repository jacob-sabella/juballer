# Repo setup checklist

One-time bootstrap for `jacob-sabella/juballer` on GitHub. Everything
below assumes the repo already exists at
`https://github.com/jacob-sabella/juballer` and that you have the
[GitHub CLI](https://cli.github.com/) installed and authenticated as
the repo owner.

## 1. Push the local repo

```bash
git remote add origin git@github.com:jacob-sabella/juballer.git
git push -u origin main
```

## 2. Lock down workflow runs from outside contributors

GitHub Actions defaults to running workflows from any contributor's
fork PR. Tighten that to "owner approval required":

`Settings → Actions → General → Fork pull request workflows from
outside collaborators` → **Require approval for all outside
collaborators**.

The `if:` guard on every workflow in `.github/workflows/` already
short-circuits fork-PR runs as a defense-in-depth, but the repo-level
setting is the canonical gate.

## 3. Branch protection on `main`

Apply the rules below — owner-only merges, required CI, no force pushes,
no deletion. Run from a clone of the repo:

```bash
gh api -X PUT \
  repos/jacob-sabella/juballer/branches/main/protection \
  --input - << 'JSON'
{
  "required_status_checks": {
    "strict": true,
    "contexts": ["Linux", "Windows"]
  },
  "enforce_admins": false,
  "required_pull_request_reviews": {
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": true,
    "required_approving_review_count": 1
  },
  "restrictions": {
    "users": ["jacob-sabella"],
    "teams": [],
    "apps": []
  },
  "required_linear_history": true,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "required_conversation_resolution": true,
  "lock_branch": false,
  "block_creations": false
}
JSON
```

Notes:

- `restrictions.users` limits who can push directly (i.e. who can merge
  via `gh pr merge --admin`) to the listed accounts. Everyone else
  must go through the PR + CI flow.
- `required_status_checks.contexts` must match the *job names* exposed
  by the CI workflow — `Linux` and `Windows` come from the `name:` field
  in `.github/workflows/ci.yml`. Update both files together.
- `enforce_admins` is `false` so the owner can land emergency hotfixes
  without bypassing branch protection — if you want even stricter
  policy, flip it to `true`.

## 4. Default branch + repository defaults

```bash
gh repo edit jacob-sabella/juballer \
  --default-branch main \
  --enable-issues \
  --enable-merge-commit=false \
  --enable-squash-merge \
  --enable-rebase-merge=false \
  --delete-branch-on-merge
```

## 5. Releases

Tag-triggered. Cut a release with:

```bash
git tag v0.1.0
git push origin v0.1.0
```

`.github/workflows/release.yml` builds Linux + Windows binaries,
attaches them to a new GitHub Release, and auto-generates release
notes from merged PRs since the last tag.

## 6. Verify

```bash
gh api repos/jacob-sabella/juballer/branches/main/protection \
  | jq '{required_status_checks, restrictions, required_pull_request_reviews}'
```

The output should show the rules above.
