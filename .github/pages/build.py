"""Build the public documentation site from the repository's Markdown."""

import logging
import os
from pathlib import Path
import posixpath
import re
import shutil
from tempfile import TemporaryDirectory

from mkdocs.commands.build import build
from mkdocs.config import load_config
from mkdocs.exceptions import Abort


PAGES = Path(__file__).resolve().parent
ROOT = PAGES.parents[1]
DOCS = ROOT / "docs"
SITE = ROOT / "target" / "pages"
REPOSITORY = "https://github.com/emojiiii/gpui-flow"

# These references remain browsable on GitHub without publishing code or CI files.
SOURCE_LINKS = {
    ".github/workflows/ci.yml": "blob",
    ".github/workflows/pages.yml": "blob",
    "crates/flow-creative/src/main.rs": "blob",
    "docs/examples/src/bin": "tree",
    "docs/examples/src/bin/plugins.rs": "blob",
    "docs/examples/src/bin/runner.rs": "blob",
}
MEDIA_TYPES = {".avif", ".gif", ".jpeg", ".jpg", ".png", ".svg", ".webp"}


def stage_content(destination):
    pages = [
        ROOT / "README.md",
        ROOT / "README.zh-CN.md",
        ROOT / "vendor/gpui_wgpu/PATCH.md",
        ROOT / "vendor/gpui_wgpu/SOURCE.md",
        *DOCS.glob("*.md"),
        *(ROOT / "crates").glob("flow-*/README.md"),
    ]
    for source in pages:
        relative = source.relative_to(ROOT)

        def rewrite_link(match):
            link, separator, fragment = match[1].partition("#")
            resolved = posixpath.normpath(posixpath.join(relative.parent.as_posix(), link))
            view = SOURCE_LINKS.get(resolved)
            if view is None:
                return match[0]
            return f"]({REPOSITORY}/{view}/main/{resolved}{separator}{fragment})"

        markdown = source.read_text(encoding="utf-8")
        # Rustdoc's ignored snippets still need to render as Rust code on the site.
        markdown = re.sub(r"^(`{3,}rust),ignore$", r"\1", markdown, flags=re.MULTILINE)
        markdown = re.sub(r"\]\(([^)\n]+)\)", rewrite_link, markdown)
        output = destination / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(markdown, encoding="utf-8")

    for directory in (DOCS / "assets", DOCS / "images"):
        for source in directory.rglob("*"):
            if source.is_file() and source.suffix.lower() in MEDIA_TYPES:
                output = destination / source.relative_to(ROOT)
                output.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, output)

    shutil.copyfile(DOCS / "leafer-task-ledger.json", destination / "docs/leafer-task-ledger.json")
    shutil.copyfile(PAGES / "mermaid.js", destination / "mermaid.js")


def main():
    logging.basicConfig(level=logging.INFO, format="%(levelname)s: %(message)s")
    SITE.parent.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory(prefix="pages-source-", dir=SITE.parent) as staged:
        stage_content(Path(staged))
        config = load_config(
            config_file=str(PAGES / "mkdocs.yml"),
            docs_dir=staged,
            site_dir=str(SITE),
            site_url=os.environ.get("PAGES_SITE_URL") or None,
            strict=True,
        )
        build(config)
    print(f"Site built in {SITE}")


if __name__ == "__main__":
    try:
        main()
    except Abort as error:
        raise SystemExit(error.format_message()) from None
