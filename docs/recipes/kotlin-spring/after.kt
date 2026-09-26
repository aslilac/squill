@Repository
class MemberRepository(private val jdbc: JdbcTemplate) {
    fun activeIn(teamId: UUID): List<Member> =
        jdbc.query(
            """
            select m.id, m.name, m.role
            from members m
            where m.team_id = ? and m.deactivated_at is null
            order by m.name
            """,
            MEMBER,
            teamId,
        )

    fun deactivate(memberId: UUID): Int =
        jdbc.update(
            """
            update members
            set deactivated_at = now()
            where id = ? and deactivated_at is null
            """,
            memberId,
        )
}
