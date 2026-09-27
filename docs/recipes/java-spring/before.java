@Repository
public class MemberRepository {
    private final JdbcTemplate jdbc;

    public MemberRepository(JdbcTemplate jdbc) {
        this.jdbc = jdbc;
    }

    public List<Member> activeIn(UUID teamId) {
        return jdbc.query("""
            SELECT m.id, m.name, m.role FROM members m
            WHERE m.team_id = ? AND m.deactivated_at IS NULL
            ORDER BY m.name
            """, MEMBER, teamId);
    }

    public int deactivate(UUID memberId) {
        return jdbc.update("""
            UPDATE members SET deactivated_at = now()
            WHERE id = ? AND deactivated_at IS NULL
            """, memberId);
    }

    public int count() {
        return jdbc.queryForObject("SELECT count(*) FROM members", Integer.class);
    }
}
