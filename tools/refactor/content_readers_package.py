#!/usr/bin/env python3
"""Move named items from the package reader modules on the #3871 base."""

from pathlib import Path
import re
from textwrap import dedent


SRC = Path(__file__).resolve().parents[2] / "crates/sunrise-mod-package/src"


class Moves:
    def __init__(self, source):
        self.path = SRC / source
        self.source = self.path.read_text()
        self.outputs = {}

    def cut(self, start, end):
        matches = list(re.finditer(start, self.source, re.MULTILINE))
        if len(matches) != 1:
            raise ValueError(f"{self.path}: expected one match for {start}, got {len(matches)}")
        (match,) = matches
        # These sources are rustfmt-formatted: use the item's closing line at its own indent.
        finish = re.search(end, self.source[match.start():], re.MULTILINE)
        if finish is None:
            raise ValueError(f"{self.path}: {start} has no terminator {end}")
        stop = match.start() + finish.end()
        cut = self.source[match.start():stop]
        self.source = self.source[:match.start()] + self.source[stop:].lstrip("\n")
        return cut.rstrip("\n")

    def items(self, *names):
        return "\n\n".join(self.cut(r"^(?:(?://[^\n]*|#\[[^\n]*\])\n)*" + name, r"^\}\n") for name in names)

    def constants(self, *names):
        return "\n\n".join(self.cut(rf"^(?:(?://[^\n]*)\n)*pub const {name}:", r";\n") for name in names)

    def methods(self, owner, *names):
        body = "\n\n".join(
            self.cut(rf"^(?:    ///[^\n]*\n)*    (?:pub )?fn {name}(?:\(|<)", r"^    \}\n")
            for name in names
        )
        return f"impl {owner} {{\n{body}\n}}"

    def into(self, target, *parts):
        path = SRC / target
        if path.exists() or path in self.outputs:
            raise ValueError(f"refusing to overwrite {path}")
        self.outputs[path] = "\n\n".join(parts) + "\n"

    def save(self):
        self.path.write_text(self.source.rstrip("\n") + "\n")
        for path, source in self.outputs.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(source)


def main():
    content = Moves("content.rs")
    content.into(
        "content/entries.rs",
        content.items(
            r"pub struct Block \{", r"pub struct Item \{", r"pub struct FluidLook \{",
            r"impl FluidLook \{", r"pub struct FluidModel \{", r"pub struct CreativeTab \{",
            r"pub struct BlockEntityType \{", r"impl RandomTicks \{",
        ),
        content.cut(r"^/// A declaration checked[^\n]*\n#\[derive\(Clone\)\]\npub struct LocalDeclaration", r";\n"),
        content.items(r"impl<T> LocalDeclaration<T> \{", r"impl Block \{", r"impl Item \{"),
    )
    content.into(
        "content/load.rs",
        content.constants("MAX_FILES", "MAX_FILE", "MAX_TOTAL"),
        content.items(
            r"pub fn check_path\(", r"enum Source \{", r"pub struct Content \{",
            r"pub enum FileReader<", r"impl FileReader<'_> \{", r"pub fn clash\(",
        ),
        content.methods(
            "Content", "read", "read_declaring", "declared", "declare", "declare_validated",
            "check_declaration_batch", "source", "is_empty", "server_only_refusal", "file", "reader",
        ),
        content.items(
            r"fn reader_of\(", r"fn open_zip\(", r"pub\(crate\) fn folder_files\(",
            r"pub\(crate\) fn runtime_folder_files\(", r"fn walk\(",
        ),
    )

    references = Moves("content/references.rs")
    references.into(
        "content/references/declarations.rs",
        references.cut(r"^(?://[^\n]*\n)*const CHECKED_TAG_FOLDERS:", r"^\];\n"),
        references.items(r"fn parse\(", r"fn conditional\("),
        references.methods(
            "References<'_>", "parsed", "json", "data", "blockstate", "read_model",
            "item_definition", "recipe", "loot_table", "tag", "names",
        ),
        references.items(r"fn type_of\(", r"fn recipe_ids<", r"fn ingredient_ids<"),
        references.cut(r"^type LootReference<", r";\n"),
        references.items(r"fn loot_table_ids<", r"fn list<", r"fn entries<", r"fn conditions<"),
    )

    manifest = Moves("manifest.rs")
    manifest.into(
        "manifest/package.rs",
        manifest.constants("MAX_USES", "STORAGE_DIR", "MAX_MANIFEST", "MAX_SOURCE_FILES", "MAX_SOURCE_BYTES", "NOT_PACKAGED"),
        manifest.methods("Manifest", "owned_namespaces", "server_only_declaration"),
        manifest.items(
            r"pub fn source_file\(", r"pub\(crate\) fn plain_path\(",
            r"pub struct Package \{", r"impl Package \{", r"fn minisign_secret_key\(",
            r"pub enum ContentFileError \{", r"impl std::fmt::Display for ContentFileError \{",
            r"impl std::error::Error for ContentFileError \{", r"pub\(crate\) fn check_file<",
            r"fn check_folder_file\(", r"pub fn find\(",
        ),
    )

    jar = Moves("manifest/jar.rs")
    jar.into(
        "manifest/jar/requirements.rs",
        jar.items(r"pub struct JarRequirement \{", r"struct RequirementFields \{", r"impl TryFrom<RequirementFields> for JarRequirement \{"),
    )
    jar.into(
        "manifest/jar/cache.rs",
        jar.items(
            r"pub struct MissingJar \{", r"impl fmt::Display for MissingJar \{",
        ),
        jar.cut(r"^impl std::error::Error for MissingJar \{\}", r"\{\}\n"),
        jar.items(
            r"pub enum JarInputsError \{", r"impl fmt::Display for JarInputsError \{",
            r"impl std::error::Error for JarInputsError \{", r"pub struct JarConflict \{",
            r"impl fmt::Display for JarConflict \{",
        ),
        jar.cut(r"^impl std::error::Error for JarConflict \{\}", r"\{\}\n"),
        jar.items(r"pub fn collect_jar_requirements<", r"pub fn missing_jars\("),
        jar.methods("JarRef", "cache_path"),
    )

    hashes = Moves("manifest/hash.rs")
    runtime = Moves("manifest/runtime_files.rs")
    data = runtime.items(r"pub struct RuntimeData \{", r"impl RuntimeData \{")
    runtime.into(
        "manifest/runtime_files/data.rs",
        runtime.constants("RUNTIME_DIR", "MAX_RUNTIME_FILE", "MAX_RUNTIME_TOTAL"),
        hashes.items(r"pub fn sha512_of\("),
        runtime.methods("RuntimeTable", "digest", "check_layout", "check"),
        data,
    )

    native = [Moves(f"manifest/{name}.rs") for name in ("loops", "mixin")]
    for file in native:
        tests = file.cut(r"^#\[cfg\(test\)\]\nmod tests \{", r"^\}\n")
        file.into(f"manifest/{file.path.stem}_tests.rs", dedent(tests.split("\n", 2)[2].removesuffix("\n}")))

    for file in (content, references, manifest, jar, hashes, runtime, *native):
        file.save()


if __name__ == "__main__":
    main()
