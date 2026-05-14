from pathlib import Path


def find_repo_root(start: Path | None = None) -> Path:
    path = Path(start or Path.cwd()).resolve()
    for candidate in [path, *path.parents]:
        if (candidate / "Cargo.toml").exists() and (candidate / "simulator").exists():
            return candidate
    raise RuntimeError("Cannot find repo root. Run the notebook from this repository.")


def first_existing(results_dir: Path, *names: str) -> Path | None:
    for name in names:
        path = results_dir / name
        if path.exists():
            return path
    return None
