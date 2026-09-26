public static List<Note> Search(SqliteConnection connection, string term)
{
    using var command = connection.CreateCommand();
    command.CommandText =
        """
        select n.id, n.title, snippet(notes_fts, 1, '[', ']', '…', 12) as excerpt
        from notes_fts join notes n on n.id = notes_fts.rowid
        where notes_fts match $term
        order by rank
        limit 50
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
