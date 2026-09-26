PGresult* overdue_invoices(PGconn* conn, const char* cutoff) {
    const char* params[] = {cutoff};
    return PQexecParams(conn,
        R"(
        select i.id, i.total, c.email
        from invoices i join customers c on c.id = i.customer_id
        where i.due_at < $1::timestamptz and i.paid_at is null
        order by i.due_at
        )",
        1, nullptr, params, nullptr, nullptr, 0);
}
