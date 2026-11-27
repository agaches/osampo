#!/usr/bin/env bash
# Suite complète (non-régression) : daemon Rust, plateforme et widget desktop C++,
# installeur, données personnelles, mod Claude Code, acceptance pytest.
set -euo pipefail
racine="$(cd "$(dirname "$0")/.." && pwd)"

(cd "$racine" && cargo test -q --workspace)

(cd "$racine/platform/board_183_sq/test" && cmake -B build -S . >/dev/null && cmake --build build -j >/dev/null && ctest --test-dir build --output-on-failure)
(cd "$racine/platform" && cmake -B build -S . >/dev/null && cmake --build build -j >/dev/null && ctest --test-dir build --output-on-failure)
(cd "$racine/widgets/osampo/desktop" && cmake -B build -S . >/dev/null && cmake --build build -j >/dev/null && ctest --test-dir build --output-on-failure)

bash "$racine/installer/linux/tests/test_installer.sh"
bash "$racine/tests/test_verifier_publication.sh"
bash "$racine/tools/verifier_publication.sh"

# Mod Claude Code : seulement si la CLI claude est installee.
if command -v claude >/dev/null; then
  (cd "$racine" && claude plugin test claude-mod)
fi

if compgen -G "$racine/tests/acceptance/test_*.py" >/dev/null; then
  python3 -m pytest -q "$racine/tests/acceptance"
fi
