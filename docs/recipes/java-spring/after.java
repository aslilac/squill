@Repository
public class MemberRepository {
    private final JdbcTemplate jdbc;

    public MemberRepository(JdbcTemplate jdbc) {
        this.jdbc = jdbc;
    }

    public List<Member> activeIn(UUID teamId) {
        return jdbc.query("""
        select m.id, m.name, m.role
        from members m
        where m.team_id = ? and m.deactivated_at is null
        order by m.name
        """, MEMBER, teamId);
    }

    public int deactivate(UUID memberId) {
        return jdbc.update("""
        update members
        set deactivated_at = now()
        where id = ? and deactivated_at is null
        """, memberId);
    }

    public int count() {
        return jdbc.queryForObject("SELECT count(*) FROM members", Integer.class);
    }
}
