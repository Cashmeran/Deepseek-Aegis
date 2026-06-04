use rusqlite::Connection;
use std::result::Result;

/// 单个数据库迁移。
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

/// 所有迁移按版本号升序排列。
/// 运行顺序: v1 → v2 → v3 → ...
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "001_init_code_graph",
        sql: include_str!("sql/001_init_code_graph.sql"),
    },
    Migration {
        version: 2,
        name: "002_init_memory",
        sql: include_str!("sql/002_init_memory.sql"),
    },
];

/// 执行所有未应用的数据库迁移。
///
/// 使用 SQLite `PRAGMA user_version` 追踪当前版本，
/// 只执行 version > current 的迁移。
pub fn run_migrations(conn: &Connection) -> Result<u32, rusqlite::Error> {
    // 启用 WAL 模式和外键约束
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

    let current: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let mut applied = 0;

    for m in MIGRATIONS.iter().filter(|m| m.version > current) {
        conn.execute_batch(m.sql)?;
        conn.pragma_update(None, "user_version", m.version)?;
        tracing::info!("Applied migration {}: {}", m.version, m.name);
        applied += 1;
    }

    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrations_are_ordered() {
        // 验证迁移列表按版本号升序排列
        for window in MIGRATIONS.windows(2) {
            assert!(
                window[0].version < window[1].version,
                "Migration '{}' (v{}) should come before '{}' (v{})",
                window[0].name,
                window[0].version,
                window[1].name,
                window[1].version,
            );
        }
    }

    #[test]
    fn test_run_migrations_on_empty_db() {
        let conn = Connection::open_in_memory().unwrap();
        let count = run_migrations(&conn).unwrap();
        assert_eq!(count, 2); // 空数据库应该应用两个迁移

        // 第二次运行应该不应用任何迁移
        let count = run_migrations(&conn).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_all_migrations_have_nonempty_sql() {
        for m in MIGRATIONS {
            assert!(!m.sql.trim().is_empty(), "Migration '{}' has empty SQL", m.name);
        }
    }

    #[test]
    fn test_migration_versions_unique() {
        let versions: Vec<u32> = MIGRATIONS.iter().map(|m| m.version).collect();
        let mut unique = versions.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(versions, unique, "Migration versions are not unique");
    }
}
