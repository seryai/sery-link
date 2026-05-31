/// A driver catalog entry that maps a driver key to a human-readable label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverCatalogEntry {
    /// Registry key — must match keys in driver-registry.json.
    pub key: &'static str,
    /// Display label shown in the Driver Store UI.
    pub label: &'static str,
    /// Whether to show in the Driver Store list.
    pub store_visible: bool,
}

const DRIVER_CATALOG: &[DriverCatalogEntry] = &[
    DriverCatalogEntry { key: "oracle", label: "Oracle", store_visible: true },
    DriverCatalogEntry { key: "oracle-legacy", label: "Oracle Legacy", store_visible: true },
    DriverCatalogEntry { key: "oracle-10g", label: "Oracle 10g", store_visible: true },
    DriverCatalogEntry { key: "snowflake", label: "Snowflake", store_visible: true },
    DriverCatalogEntry { key: "db2", label: "IBM DB2", store_visible: true },
    DriverCatalogEntry { key: "informix", label: "IBM Informix", store_visible: true },
    DriverCatalogEntry { key: "saphana", label: "SAP HANA", store_visible: true },
    DriverCatalogEntry { key: "teradata", label: "Teradata", store_visible: true },
    DriverCatalogEntry { key: "vertica", label: "Vertica", store_visible: true },
    DriverCatalogEntry { key: "databricks", label: "Databricks SQL", store_visible: true },
    DriverCatalogEntry { key: "trino", label: "Trino (Presto)", store_visible: true },
    DriverCatalogEntry { key: "hive", label: "Apache Hive", store_visible: true },
    DriverCatalogEntry { key: "bigquery", label: "Google BigQuery", store_visible: true },
    DriverCatalogEntry { key: "cassandra", label: "Apache Cassandra", store_visible: true },
    DriverCatalogEntry { key: "neo4j", label: "Neo4j", store_visible: true },
    DriverCatalogEntry { key: "firebird", label: "Firebird", store_visible: true },
    DriverCatalogEntry { key: "exasol", label: "Exasol", store_visible: true },
    DriverCatalogEntry { key: "h2", label: "H2", store_visible: true },
    DriverCatalogEntry { key: "kylin", label: "Apache Kylin", store_visible: true },
    DriverCatalogEntry { key: "access", label: "Microsoft Access", store_visible: true },
    DriverCatalogEntry { key: "dameng", label: "Dameng DM8", store_visible: true },
    DriverCatalogEntry { key: "kingbase", label: "KingbaseES", store_visible: true },
    DriverCatalogEntry { key: "highgo", label: "HighGo", store_visible: true },
    DriverCatalogEntry { key: "vastbase", label: "Vastbase", store_visible: true },
    DriverCatalogEntry { key: "goldendb", label: "GoldenDB", store_visible: true },
    DriverCatalogEntry { key: "oceanbase-oracle", label: "OceanBase Oracle Mode", store_visible: true },
    DriverCatalogEntry { key: "gbase", label: "GBase", store_visible: true },
    DriverCatalogEntry { key: "sundb", label: "SunDB", store_visible: true },
    DriverCatalogEntry { key: "yashandb", label: "YashanDB", store_visible: true },
    DriverCatalogEntry { key: "tdengine", label: "TDengine", store_visible: true },
    DriverCatalogEntry { key: "xugu", label: "XuguDB", store_visible: true },
    DriverCatalogEntry { key: "mongodb", label: "MongoDB (Legacy)", store_visible: true },
    DriverCatalogEntry { key: "iris", label: "InterSystems IRIS", store_visible: false },
];

pub fn entries() -> &'static [DriverCatalogEntry] {
    DRIVER_CATALOG
}

/// Returns an iterator over (key, label) pairs for the driver store UI.
pub fn driver_store_entries() -> impl Iterator<Item = (&'static str, &'static str)> {
    entries().iter().filter(|e| e.store_visible).map(|e| (e.key, e.label))
}

/// Look up a human-readable label for a driver key.
pub fn label_for_key(key: &str) -> Option<&'static str> {
    entries().iter().find(|e| e.key == key).map(|e| e.label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_enterprise_drivers() {
        let keys: Vec<&str> = entries().iter().map(|e| e.key).collect();
        assert!(keys.contains(&"oracle"));
        assert!(keys.contains(&"snowflake"));
        assert!(keys.contains(&"db2"));
        assert!(keys.contains(&"saphana"));
    }

    #[test]
    fn driver_store_entries_excludes_hidden() {
        let visible: Vec<&str> = driver_store_entries().map(|(k, _)| k).collect();
        assert!(!visible.contains(&"iris")); // store_visible: false
        assert!(visible.contains(&"oracle"));
    }

    #[test]
    fn label_for_key_returns_known_label() {
        assert_eq!(label_for_key("snowflake"), Some("Snowflake"));
        assert_eq!(label_for_key("unknown-driver"), None);
    }
}
