from __future__ import annotations

import json
import os
import re
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - used on Python 3.8-3.10
    import tomli as tomllib


@dataclass(frozen=True)
class MigrationConfig:
    converter: dict[str, Any] = field(default_factory=dict)
    file_map: dict[str, str] = field(default_factory=dict)


def load_migration_config(path: str | Path | None) -> MigrationConfig:
    if path is None:
        return MigrationConfig()
    config_path = Path(path)
    if not config_path.exists():
        raise FileNotFoundError(config_path)
    loaded = tomllib.loads(config_path.read_text(encoding="utf-8"))
    converter = dict(loaded.get("converter", loaded))
    converter.pop("file_map", None)
    converter.pop("dd", None)
    return MigrationConfig(
        converter=converter,
        file_map=_file_map_from_config(loaded),
    )


def build_file_map(
    mapping: dict[str, str], *, source_root: str | Path | None = None
) -> dict[str, str]:
    root = Path(source_root).resolve() if source_root else None
    out: dict[str, str] = {}
    for raw_name, raw_path in mapping.items():
        name = str(raw_name).strip()
        if not name:
            raise ValueError("DD/file-map names must not be empty")
        path = Path(str(raw_path)).expanduser()
        if not path.is_absolute() and root is not None:
            path = root / path
        out[name] = str(path)
    return out


def project_output_dir(output_root: str | Path, source_name: str) -> Path:
    stem = Path(source_name).with_suffix("").as_posix()
    safe = re.sub(r"[^A-Za-z0-9_.-]+", "-", stem).strip(".-")
    return Path(output_root) / (safe or "program")


def write_generated_file_map(project_dir: str | Path, mapping: dict[str, str]) -> Path | None:
    if not mapping:
        return None
    project = Path(project_dir)
    project.mkdir(parents=True, exist_ok=True)
    path = project / "cobol-file-map.json"
    _write_text_atomic(path, json.dumps(mapping, indent=2, sort_keys=True))
    return path


def _write_text_atomic(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temp_path_text = tempfile.mkstemp(
        prefix=f".{path.name}.",
        suffix=".tmp",
        dir=path.parent,
        text=True,
    )
    temp_path = Path(temp_path_text)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(text)
        os.replace(temp_path, path)
    except Exception:
        try:
            temp_path.unlink()
        except FileNotFoundError:
            pass
        raise


def _file_map_from_config(config: dict[str, Any]) -> dict[str, str]:
    mapping: dict[str, str] = {}
    for key, value in dict(config.get("file_map", {})).items():
        mapping[str(key)] = str(value)
    dd_entries = config.get("dd", [])
    if isinstance(dd_entries, dict):
        dd_entries = [dd_entries]
    for entry in dd_entries:
        if not isinstance(entry, dict):
            continue
        name = entry.get("name") or entry.get("dd") or entry.get("ddname")
        path = entry.get("path") or entry.get("file")
        if name is not None and path is not None:
            mapping[str(name)] = str(path)
    return mapping
