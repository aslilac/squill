class AddSearchToArticles < ActiveRecord::Migration[7.2]
  def up
    execute <<~SQL
    alter table articles
      add column search tsvector generated always as (
        to_tsvector('english', coalesce(title, '') || ' ' || coalesce(body, ''))
      ) stored;
    create index articles_search
      on articles
      using gin(search)
    SQL
  end
end

class Article < ApplicationRecord
  def self.search(term)
    find_by_sql([<<~SQL, term])
    select id, title
    from articles
    where search @@ plainto_tsquery('english', ?)
    order by published_at desc
    SQL
  end

  def self.monthly_counts
    rows = connection.select_rows(<<~SQL)
    select date_trunc('month', published_at) as month, count(*)
    from articles
    where published_at is not null
    group by 1
    order by 1
    SQL
    rows.to_h
  end

  def self.by_author(name)
    find_by_sql(<<~SQL)
      SELECT * FROM articles WHERE author = '#{name}'
    SQL
  end
end
