use chrono::{DateTime, Duration, TimeZone, Utc};
use plist::Value as PlistValue;
use rusqlite::{params_from_iter, types::Value as SqlValue, Connection, OpenFlags, Row};
use serde::Serialize;
use std::collections::{hash_map::DefaultHasher, HashMap, VecDeque};
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const MAX_NOTIFICATION_RECORD_CACHE_ENTRIES: usize = 1_000;
static NOTIFICATION_RECORD_CACHE: OnceLock<Mutex<NotificationRecordCache>> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRecord {
    pub id: i64,
    pub app_identifier: String,
    pub app_name: String,
    pub delivered_at: DateTime<Utc>,
    pub title: String,
    pub subtitle: String,
    pub body: String,
}

struct NotificationPayload {
    title: String,
    subtitle: String,
    body: String,
}

#[derive(Default)]
struct NotificationRecordCache {
    records: HashMap<i64, CachedNotificationRecord>,
    order: VecDeque<i64>,
}

#[derive(Clone)]
struct CachedNotificationRecord {
    app_identifier: String,
    delivered_date: f64,
    payload_hash: u64,
    record: NotificationRecord,
}

pub fn notification_database_path() -> Result<PathBuf, Box<dyn Error>> {
    let home = dirs::home_dir().ok_or("无法读取用户主目录")?;
    Ok(home.join("Library/Group Containers/group.com.apple.usernoted/db2/db"))
}

pub fn notification_database_corrupt_backup_path() -> Result<PathBuf, Box<dyn Error>> {
    let path = notification_database_path()?;
    Ok(path.with_file_name("db.corrupt"))
}

pub fn record_count() -> Result<i64, Box<dyn Error>> {
    let connection = open_notification_connection()?;
    let count = connection.query_row("select count(*) from record;", [], |row| {
        row.get::<_, i64>(0)
    })?;
    Ok(count)
}

pub fn recent_records(limit: usize) -> Result<Vec<NotificationRecord>, Box<dyn Error>> {
    let connection = open_notification_connection()?;

    let mut statement = connection.prepare(
        r#"
        select r.rec_id, coalesce(a.identifier, ''), coalesce(r.delivered_date, r.request_date, 0), r.data
        from record r
        left join app a on r.app_id = a.app_id
        order by r.rec_id desc
        limit ?1;
        "#,
    )?;

    let rows = statement.query_map([limit as i64], notification_record_from_row)?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    records.reverse();
    Ok(records)
}

pub fn recent_records_excluding(
    limit: usize,
    ignored_app_identifiers: &[String],
) -> Result<Vec<NotificationRecord>, Box<dyn Error>> {
    let ignored_app_identifiers = normalized_ignored_app_identifiers(ignored_app_identifiers);
    if ignored_app_identifiers.is_empty() {
        return recent_records(limit);
    }

    let placeholders = std::iter::repeat_n("?", ignored_app_identifiers.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        select r.rec_id, coalesce(a.identifier, ''), coalesce(r.delivered_date, r.request_date, 0), r.data
        from record r
        left join app a on r.app_id = a.app_id
        where lower(coalesce(a.identifier, '')) not in ({placeholders})
        order by r.rec_id desc
        limit ?;
        "#
    );
    let mut params = ignored_app_identifiers
        .into_iter()
        .map(SqlValue::Text)
        .collect::<Vec<_>>();
    params.push(SqlValue::Integer(limit as i64));

    let connection = open_notification_connection()?;
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(params), notification_record_from_row)?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    records.reverse();
    Ok(records)
}

pub fn recent_records_including(
    limit: usize,
    app_identifiers: &[String],
) -> Result<Vec<NotificationRecord>, Box<dyn Error>> {
    let app_identifiers = normalized_ignored_app_identifiers(app_identifiers);
    if app_identifiers.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat_n("?", app_identifiers.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        select r.rec_id, coalesce(a.identifier, ''), coalesce(r.delivered_date, r.request_date, 0), r.data
        from record r
        left join app a on r.app_id = a.app_id
        where lower(coalesce(a.identifier, '')) in ({placeholders})
        order by r.rec_id desc
        limit ?;
        "#
    );
    let mut params = app_identifiers
        .into_iter()
        .map(SqlValue::Text)
        .collect::<Vec<_>>();
    params.push(SqlValue::Integer(limit as i64));

    let connection = open_notification_connection()?;
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(params), notification_record_from_row)?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    records.reverse();
    Ok(records)
}

pub fn max_record_id() -> Result<i64, Box<dyn Error>> {
    let connection = open_notification_connection()?;
    let id = connection.query_row("select coalesce(max(rec_id), 0) from record;", [], |row| {
        row.get::<_, i64>(0)
    })?;
    Ok(id)
}

pub fn records_after(
    record_id: i64,
    limit: usize,
) -> Result<Vec<NotificationRecord>, Box<dyn Error>> {
    let connection = open_notification_connection()?;

    let mut statement = connection.prepare(
        r#"
        select r.rec_id, coalesce(a.identifier, ''), coalesce(r.delivered_date, r.request_date, 0), r.data
        from record r
        left join app a on r.app_id = a.app_id
        where r.rec_id > ?1
        order by r.rec_id asc
        limit ?2;
        "#,
    )?;

    let rows = statement.query_map([record_id, limit as i64], notification_record_from_row)?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

pub fn record_by_id(record_id: i64) -> Result<Option<NotificationRecord>, Box<dyn Error>> {
    let connection = open_notification_connection()?;
    let mut statement = connection.prepare(
        r#"
        select r.rec_id, coalesce(a.identifier, ''), coalesce(r.delivered_date, r.request_date, 0), r.data
        from record r
        left join app a on r.app_id = a.app_id
        where r.rec_id = ?1
        limit 1;
        "#,
    )?;
    let mut rows = statement.query_map([record_id], notification_record_from_row)?;
    rows.next().transpose().map_err(Into::into)
}

pub fn delete_record(record_id: i64) -> Result<usize, Box<dyn Error>> {
    let path = notification_database_path()?;
    let mut connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )?;
    connection.busy_timeout(std::time::Duration::from_millis(1_000))?;
    let transaction = connection.transaction()?;
    let associated_rows = delete_rows_referencing_record(&transaction, record_id)?;
    let record_rows = transaction.execute("delete from record where rec_id = ?1", [record_id])?;
    transaction.commit()?;
    Ok(record_rows + associated_rows)
}

fn delete_rows_referencing_record(
    connection: &Connection,
    record_id: i64,
) -> Result<usize, Box<dyn Error>> {
    let mut statement = connection.prepare(
        r#"
        select name
        from sqlite_master
        where type = 'table'
          and name not like 'sqlite_%'
          and name <> 'record'
        "#,
    )?;
    let tables = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut changed = 0;
    for table in tables {
        for column in record_foreign_key_reference_columns(connection, &table)? {
            let sql = format!(
                "delete from {} where {} = ?1",
                quote_sql_identifier(&table),
                quote_sql_identifier(&column)
            );
            changed += connection.execute(&sql, [record_id])?;
        }
    }
    Ok(changed)
}

fn record_foreign_key_reference_columns(
    connection: &Connection,
    table: &str,
) -> Result<Vec<String>, Box<dyn Error>> {
    let sql = format!("pragma foreign_key_list({})", quote_sql_identifier(table));
    let mut statement = connection.prepare(&sql)?;
    let references = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(references
        .into_iter()
        .filter_map(|(foreign_table, from_column, to_column)| {
            (foreign_table == "record" && (to_column.is_empty() || to_column == "rec_id"))
                .then_some(from_column)
        })
        .collect())
}

fn quote_sql_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn open_notification_connection() -> Result<Connection, Box<dyn Error>> {
    let path = notification_database_path()?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )?;
    connection.busy_timeout(std::time::Duration::from_millis(1_000))?;
    Ok(connection)
}

fn notification_record_from_row(row: &Row<'_>) -> rusqlite::Result<NotificationRecord> {
    let id: i64 = row.get(0)?;
    let app_identifier: String = row.get(1)?;
    let delivered_date: f64 = row.get(2)?;
    let data: Vec<u8> = row.get(3).unwrap_or_default();
    let payload_hash = notification_payload_hash(&data);
    if let Some(record) =
        cached_notification_record(id, &app_identifier, delivered_date, payload_hash)
    {
        return Ok(record);
    }
    let payload = parse_payload(&data);

    let record = NotificationRecord {
        id,
        app_name: app_identifier.clone(),
        app_identifier,
        delivered_at: apple_reference_date(delivered_date),
        title: payload.title,
        subtitle: payload.subtitle,
        body: payload.body,
    };
    remember_notification_record(delivered_date, payload_hash, record.clone());
    Ok(record)
}

fn cached_notification_record(
    id: i64,
    app_identifier: &str,
    delivered_date: f64,
    payload_hash: u64,
) -> Option<NotificationRecord> {
    let Ok(mut cache) = NOTIFICATION_RECORD_CACHE
        .get_or_init(|| Mutex::new(NotificationRecordCache::default()))
        .lock()
    else {
        return None;
    };
    let cached = cache.records.get(&id)?;
    if cached.app_identifier != app_identifier
        || cached.delivered_date != delivered_date
        || cached.payload_hash != payload_hash
    {
        return None;
    }
    let record = cached.record.clone();
    cache.order.retain(|cached_id| *cached_id != id);
    cache.order.push_back(id);
    Some(record)
}

fn remember_notification_record(
    delivered_date: f64,
    payload_hash: u64,
    record: NotificationRecord,
) {
    let Ok(mut cache) = NOTIFICATION_RECORD_CACHE
        .get_or_init(|| Mutex::new(NotificationRecordCache::default()))
        .lock()
    else {
        return;
    };
    if cache.records.contains_key(&record.id) {
        cache.order.retain(|id| *id != record.id);
    }
    let id = record.id;
    cache.order.push_back(id);
    cache.records.insert(
        id,
        CachedNotificationRecord {
            app_identifier: record.app_identifier.clone(),
            delivered_date,
            payload_hash,
            record,
        },
    );
    while cache.records.len() > MAX_NOTIFICATION_RECORD_CACHE_ENTRIES {
        let Some(oldest_id) = cache.order.pop_front() else {
            break;
        };
        cache.records.remove(&oldest_id);
    }
}

fn notification_payload_hash(data: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

fn apple_reference_date(seconds: f64) -> DateTime<Utc> {
    let reference = Utc
        .with_ymd_and_hms(2001, 1, 1, 0, 0, 0)
        .single()
        .unwrap_or_else(Utc::now);
    reference + Duration::milliseconds((seconds * 1_000.0) as i64)
}

fn parse_payload(data: &[u8]) -> NotificationPayload {
    let Ok(value) = PlistValue::from_reader(Cursor::new(data)) else {
        return NotificationPayload {
            title: String::new(),
            subtitle: String::new(),
            body: String::new(),
        };
    };

    let request = value
        .as_dictionary()
        .and_then(|root| root.get("req"))
        .and_then(PlistValue::as_dictionary);
    let root = value.as_dictionary();

    let title = request
        .and_then(|dict| dict.get("titl"))
        .and_then(PlistValue::as_string)
        .or_else(|| {
            root.and_then(|dict| dict.get("titl"))
                .and_then(PlistValue::as_string)
        })
        .unwrap_or_default()
        .trim()
        .to_string();
    let subtitle = request
        .and_then(|dict| dict.get("subt"))
        .and_then(PlistValue::as_string)
        .or_else(|| {
            root.and_then(|dict| dict.get("subt"))
                .and_then(PlistValue::as_string)
        })
        .unwrap_or_default()
        .trim()
        .to_string();
    let body = request
        .and_then(|dict| dict.get("body"))
        .and_then(PlistValue::as_string)
        .or_else(|| {
            root.and_then(|dict| dict.get("body"))
                .and_then(PlistValue::as_string)
        })
        .unwrap_or_default()
        .trim()
        .to_string();

    NotificationPayload {
        title,
        subtitle,
        body,
    }
}

fn normalized_ignored_app_identifiers(ignored_app_identifiers: &[String]) -> Vec<String> {
    let mut items = ignored_app_identifiers
        .iter()
        .map(|item| item.trim().to_lowercase())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();
    items.sort();
    items.dedup();
    items
}

#[cfg(test)]
mod tests {
    use super::{
        apple_reference_date, cached_notification_record, delete_rows_referencing_record,
        normalized_ignored_app_identifiers, notification_payload_hash, parse_payload,
        remember_notification_record, NotificationRecord,
    };
    use rusqlite::Connection;

    #[test]
    fn normalizes_ignored_app_identifiers_for_sql_filtering() {
        let items = vec![
            " com.apple.mail ".to_string(),
            "COM.APPLE.MAIL".to_string(),
            "".to_string(),
            "com.example.App".to_string(),
        ];

        assert_eq!(
            normalized_ignored_app_identifiers(&items),
            vec!["com.apple.mail".to_string(), "com.example.app".to_string()]
        );
    }

    #[test]
    fn parses_notification_payload_from_plist() {
        let payload = parse_payload(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>req</key>
  <dict>
    <key>titl</key>
    <string>  Title  </string>
    <key>subt</key>
    <string>Subtitle</string>
    <key>body</key>
    <string>Body</string>
  </dict>
</dict>
</plist>"#,
        );

        assert_eq!(payload.title, "Title");
        assert_eq!(payload.subtitle, "Subtitle");
        assert_eq!(payload.body, "Body");
    }

    #[test]
    fn parses_payload_root_fallback_fields() {
        let payload = parse_payload(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>titl</key>
  <string>Title</string>
  <key>body</key>
  <string>Body</string>
</dict>
</plist>"#,
        );

        assert_eq!(payload.title, "Title");
        assert_eq!(payload.subtitle, "");
        assert_eq!(payload.body, "Body");
    }

    #[test]
    fn notification_record_cache_requires_matching_signature() {
        let delivered_date = 12345.0;
        let record = NotificationRecord {
            id: 9_000_001,
            app_identifier: "com.example.App".to_string(),
            app_name: "Example".to_string(),
            delivered_at: apple_reference_date(delivered_date),
            title: "Title".to_string(),
            subtitle: String::new(),
            body: "Body".to_string(),
        };

        let payload_hash = notification_payload_hash(b"payload-v1");
        remember_notification_record(delivered_date, payload_hash, record);

        assert!(cached_notification_record(
            9_000_001,
            "com.example.App",
            delivered_date,
            payload_hash
        )
        .is_some());
        assert!(cached_notification_record(
            9_000_001,
            "com.example.Other",
            delivered_date,
            payload_hash
        )
        .is_none());
        assert!(cached_notification_record(
            9_000_001,
            "com.example.App",
            delivered_date + 1.0,
            payload_hash
        )
        .is_none());
        assert!(cached_notification_record(
            9_000_001,
            "com.example.App",
            delivered_date,
            notification_payload_hash(b"payload-v2")
        )
        .is_none());
    }

    #[test]
    fn does_not_delete_rows_from_tables_that_only_look_like_record_references() {
        let connection = Connection::open_in_memory().expect("in-memory db should open");
        connection
            .execute_batch(
                r#"
                create table record_related(rec_id integer, value text);
                create table record_id_related(record_id integer, value text);
                create table app_blob(app_id integer primary key, list blob);
                insert into record_related values (10, 'remove'), (11, 'keep');
                insert into record_id_related values (10, 'remove'), (12, 'keep');
                insert into app_blob values (10, X'00');
                "#,
            )
            .expect("schema should be created");

        let changed = delete_rows_referencing_record(&connection, 10)
            .expect("plain columns should be ignored");

        assert_eq!(changed, 0);
        assert_eq!(
            connection
                .query_row("select count(*) from record_related", [], |row| row
                    .get::<_, i64>(0))
                .expect("count should be readable"),
            2
        );
        assert_eq!(
            connection
                .query_row("select count(*) from record_id_related", [], |row| row
                    .get::<_, i64>(0))
                .expect("count should be readable"),
            2
        );
        assert_eq!(
            connection
                .query_row("select count(*) from app_blob", [], |row| row
                    .get::<_, i64>(0))
                .expect("count should be readable"),
            1
        );
    }

    #[test]
    fn deletes_rows_from_tables_with_record_foreign_keys() {
        let connection = Connection::open_in_memory().expect("in-memory db should open");
        connection
            .execute_batch(
                r#"
                create table record(rec_id integer primary key);
                create table record_related(rec_id integer references record(rec_id), value text);
                create table record_id_related(record_id integer, value text);
                insert into record values (10), (11), (12);
                insert into record_related values (10, 'remove'), (11, 'keep');
                insert into record_id_related values (10, 'keep');
                "#,
            )
            .expect("schema should be created");

        let changed = delete_rows_referencing_record(&connection, 10)
            .expect("foreign-key rows should be deleted");

        assert_eq!(changed, 1);
        assert_eq!(
            connection
                .query_row("select count(*) from record_related", [], |row| row
                    .get::<_, i64>(0))
                .expect("count should be readable"),
            1
        );
        assert_eq!(
            connection
                .query_row("select count(*) from record_id_related", [], |row| row
                    .get::<_, i64>(0))
                .expect("count should be readable"),
            1
        );
    }
}
