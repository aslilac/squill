PGresult* overdue_invoices(PGconn* conn, const char* cutoff) {
    const char* params[] = {cutoff};
    return PQexecParams(conn,
        R"(
            SELECT i.id, i.total, c.email FROM invoices i
            JOIN customers c ON c.id = i.customer_id
            WHERE i.due_at < $1::timestamptz AND i.paid_at IS NULL
            ORDER BY i.due_at
        )",
        1, nullptr, params, nullptr, nullptr, 0);
}
