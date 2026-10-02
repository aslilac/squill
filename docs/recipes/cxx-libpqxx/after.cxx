std::vector<Member> active_members(pqxx::connection& conn, int team_id) {
    pqxx::work tx{conn};
    std::vector<Member> members;
    for (auto [id, name, role] : tx.query<int, std::string, std::string>(
             R"(
             select m.id, m.name, m.role
             from members m
             where m.team_id = $1 and m.deactivated_at is null
             order by m.name
             )",
             pqxx::params{team_id})) {
        members.push_back({id, std::move(name), std::move(role)});
    }
    tx.exec(
        R"(
        update teams set last_listed_at = now() where id = $1
        )",
        pqxx::params{team_id});
    tx.commit();
    return members;
}
