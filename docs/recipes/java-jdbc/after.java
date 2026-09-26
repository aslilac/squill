public List<Invoice> overdue(Connection conn, Instant cutoff) throws SQLException {
    try (var stmt = conn.prepareStatement(
            """
            select i.id, i.total, c.email
            from invoices i join customers c on c.id = i.customer_id
            where i.due_at < ? and i.paid_at is null
            order by i.due_at
            """)) {
        stmt.setTimestamp(1, Timestamp.from(cutoff));
        var invoices = new ArrayList<Invoice>();
        try (var rows = stmt.executeQuery()) {
            while (rows.next()) {
                invoices.add(Invoice.from(rows));
            }
        }
        return invoices;
    }
}
