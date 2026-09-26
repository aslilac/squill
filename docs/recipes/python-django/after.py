from django.db import connection

from .models import Article


def popular(days=30):
    return Article.objects.raw(
        """
        select a.id, a.title, count(v.id) as views
        from blog_article a join blog_view v on v.article_id = a.id
        where v.viewed_at > now() - make_interval(days => %s)
        group by a.id
        order by views desc
        """,
        [days],
    )


def purge_drafts(author_id, cutoff):
    with connection.cursor() as cursor:
        cursor.execute(
            """
            delete from blog_article
            where author_id = %s and status = 'draft' and updated_at < %s
            """,
            [author_id, cutoff],
        )
        return cursor.rowcount
