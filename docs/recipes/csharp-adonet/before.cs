public static List<Note> Search(SqliteConnection connection, string term)
{
    using var command = connection.CreateCommand();
    command.CommandText =
        """
        SELECT n.id, n.title, snippet(notes_fts, 1, '[', ']', '…', 12) AS excerpt
            FROM notes_fts JOIN notes n ON n.id = notes_fts.rowid
            WHERE notes_fts MATCH $term ORDER BY rank
            LIMIT 50
        """;
    command.Parameters.AddWithValue("$term", term);

    var notes = new List<Note>();
    using var reader = command.ExecuteReader();
    while (reader.Read())
    {
        notes.Add(new Note(reader.GetInt64(0), reader.GetString(1), reader.GetString(2)));
    }
    return notes;
}
