fn main() {
    let sqlean = "vendor/sqlean/src";

    // libsqlite3-sys exports its include path via cargo:include metadata.
    // This gives us sqlite3.h and sqlite3ext.h from rusqlite's bundled SQLite.
    let sqlite3_include =
        std::env::var("DEP_SQLITE3_INCLUDE").expect("DEP_SQLITE3_INCLUDE not set by libsqlite3-sys; is rusqlite with 'bundled' feature a dependency?");

    cc::Build::new()
        // Entry points for each extension
        .file(format!("{sqlean}/sqlite3-fuzzy.c"))
        .file(format!("{sqlean}/sqlite3-stats.c"))
        .file(format!("{sqlean}/sqlite3-crypto.c"))
        // Fuzzy extension sources
        .file(format!("{sqlean}/fuzzy/caver.c"))
        .file(format!("{sqlean}/fuzzy/common.c"))
        .file(format!("{sqlean}/fuzzy/damlev.c"))
        .file(format!("{sqlean}/fuzzy/editdist.c"))
        .file(format!("{sqlean}/fuzzy/extension.c"))
        .file(format!("{sqlean}/fuzzy/hamming.c"))
        .file(format!("{sqlean}/fuzzy/jarowin.c"))
        .file(format!("{sqlean}/fuzzy/leven.c"))
        .file(format!("{sqlean}/fuzzy/osadist.c"))
        .file(format!("{sqlean}/fuzzy/phonetic.c"))
        .file(format!("{sqlean}/fuzzy/rsoundex.c"))
        .file(format!("{sqlean}/fuzzy/soundex.c"))
        .file(format!("{sqlean}/fuzzy/translit.c"))
        // Stats extension sources
        .file(format!("{sqlean}/stats/extension.c"))
        .file(format!("{sqlean}/stats/scalar.c"))
        .file(format!("{sqlean}/stats/series.c"))
        // Crypto extension sources
        .file(format!("{sqlean}/crypto/base32.c"))
        .file(format!("{sqlean}/crypto/base64.c"))
        .file(format!("{sqlean}/crypto/base85.c"))
        .file(format!("{sqlean}/crypto/blake3.c"))
        .file(format!("{sqlean}/crypto/extension.c"))
        .file(format!("{sqlean}/crypto/hex.c"))
        .file(format!("{sqlean}/crypto/md5.c"))
        .file(format!("{sqlean}/crypto/sha1.c"))
        .file(format!("{sqlean}/crypto/sha2.c"))
        .file(format!("{sqlean}/crypto/url.c"))
        .file(format!("{sqlean}/crypto/xxhash.c"))
        // Include paths: sqlean src root (for "fuzzy/extension.h" etc.) and
        // libsqlite3-sys headers (for "sqlite3ext.h" / "sqlite3.h")
        .include(sqlean)
        .include(&sqlite3_include)
        // SQLITE_CORE tells sqlite3ext.h to resolve symbols from the host
        // process (rusqlite's bundled SQLite) rather than a dynamic library.
        .define("SQLITE_CORE", None)
        .warnings(false)
        .compile("sqlean_extensions");
}
