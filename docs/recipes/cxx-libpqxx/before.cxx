std::vector<Member> active_members(pqxx::connection& conn, int team_id) {
    pqxx::work tx{conn};
    std::vector<Member> members;
    for (auto [id, name, role] : tx.query<int, std::string, std::string>(
             R"(
                 SELECT m.id, m.name, m.role FROM members m
                 WHERE m.team_id = $1 AND m.deactivated_at IS NULL
                 ORDER BY m.name
             )",
             pqxx::params{team_id})) {
        members.push_back({id, std::move(name), std::move(role)});
    }
    tx.exec(
        R"(
            UPDATE teams SET last_listed_at = now() WHERE id = $1
        )",
        pqxx::params{team_id});
    tx.commit();
    return members;
}
