fun topPosters(since: LocalDate): List<Pair<String, Long>> = transaction {
    exec(
        """
        select u.name, count(*) as posts
        from posts p join users u on u.id = p.author_id
        where p.created_at >= ?
        group by u.name
        order by posts desc
        limit 10
        """.trimIndent(),
        listOf(JavaLocalDateColumnType() to since),
    ) { rs ->
        generateSequence { if (rs.next()) rs.getString(1) to rs.getLong(2) else null }.toList()
    }.orEmpty()
}

fun archiveOldPosts() = transaction {
    exec(
        """
        update posts
        set archived = true
        where created_at < now() - interval '2 years'
        """
    )
}
