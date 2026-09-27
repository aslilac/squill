fun topPosters(since: LocalDate): List<Pair<String, Long>> = transaction {
    exec(
        """
        SELECT u.name, count(*) AS posts FROM posts p
            JOIN users u ON u.id = p.author_id WHERE p.created_at >= ?
            GROUP BY u.name ORDER BY posts DESC LIMIT 10
        """.trimIndent(),
        listOf(JavaLocalDateColumnType() to since),
    ) { rs ->
        generateSequence { if (rs.next()) rs.getString(1) to rs.getLong(2) else null }.toList()
    }.orEmpty()
}

fun archiveOldPosts() = transaction {
    exec(
        """
        UPDATE posts SET archived = true WHERE created_at < now() - interval '2 years'
        """
    )
}
