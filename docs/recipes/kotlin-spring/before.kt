@Repository
class MemberRepository(private val jdbc: JdbcTemplate) {
    fun activeIn(teamId: UUID): List<Member> =
        jdbc.query(
            """
            SELECT m.id, m.name, m.role FROM members m
                WHERE m.team_id = ? AND m.deactivated_at IS NULL
                ORDER BY m.name
            """,
            MEMBER,
            teamId,
        )

    fun deactivate(memberId: UUID): Int =
        jdbc.update(
            """
            UPDATE members SET deactivated_at = now()
                WHERE id = ? AND deactivated_at IS NULL
            """,
            memberId,
        )
}
