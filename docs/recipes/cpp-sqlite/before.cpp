std::vector<Note> recent_notes(sqlite3* db, std::int64_t since) {
    sqlite3_stmt* stmt = nullptr;
    int rc = sqlite3_prepare_v2(
        db,
        R"sql(
            SELECT id, title, updated_at FROM notes WHERE updated_at > ?1 AND archived = 0 ORDER BY updated_at DESC LIMIT 50
        )sql",
        -1, &stmt, nullptr);
    if (rc != SQLITE_OK) throw sqlite_error(db);
    sqlite3_bind_int64(stmt, 1, since);

    std::vector<Note> notes;
    while (sqlite3_step(stmt) == SQLITE_ROW) {
        notes.push_back(Note::from_row(stmt));
    }
    sqlite3_finalize(stmt);
    return notes;
}
