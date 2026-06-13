I actually think you’re pointing at the most interesting part of ecommerce search.

Many search engineers come from Lucene/Elasticsearch and assume:

BM25
  +
some boosts
  +
facets

is the center of the universe.

But if your goal is:

Long tail
Discovery
Recommendations
Natural language
AI shopping assistant

then your experience with Google’s vector search is completely expected.

In fact, some of the largest ecommerce players have published results showing dense retrieval significantly improves long-tail queries because those queries have little behavioral data and poor lexical overlap. Best Buy reported improvements by introducing embedding-based retrieval specifically for long-tail ecommerce searches.  ￼

⸻

The key question is not “BM25 or vectors”

The real question is:

What are users searching for?

Head queries

iphone 17
airpods
nike air max

BM25 is almost impossible to beat.

⸻

Long-tail queries

gift for a 12 year old that likes astronomy
shoe for standing all day in a hospital
keyboard for programmer with wrist pain

Dense retrieval wins.

The query and product descriptions may share almost no words.

Yet embeddings understand intent.  ￼

⸻

If I were building a new search engine in 2026

Knowing your previous search work, I would not start from Lucene.

I would start from:

Catalog
    |
Embeddings
    |
ANN Search
    |
Reranking

and treat keyword search as a feature.

Not the opposite.

⸻

What I think you’re really trying to build

Not search.

Something closer to:

Intent Engine

User says:

my wife likes yoga and coffee
birthday gift under R$200

System returns:

yoga mat
coffee subscription
insulated mug
foam roller

There is no lexical match.

This is recommendation disguised as search.

⸻

Where TurboVec becomes interesting

Now we’re talking.

Suppose:

10 million products
1024 dimension embeddings

Raw storage:

~40GB+

Per tenant.

That becomes painful.

TurboQuant/TurboVec tries to reduce that dramatically while maintaining recall. That’s where it helps. It is a storage and retrieval optimization layer, not the intelligence layer.  ￼

Think:

Embeddings
      ↓
TurboVec
      ↓
ANN Search

not:

TurboVec
      ↓
Magic ecommerce search

⸻

What excites me more than TurboVec

This:

Product embeddings

Instead of embedding only:

{
  "title": "Nike Air Max"
}

embed:

{
  "title":"Nike Air Max",
  "brand":"Nike",
  "category":"Running Shoes",
  "gender":"Male",
  "material":"Mesh",
  "description":"..."
}

Now your vector contains product semantics.

⸻

User embeddings

Build vectors from:

clicks
views
purchases
wishlists

Now:

User vector
+
Query vector
+
Product vector

becomes your retrieval space.

That is much closer to what Amazon, TikTok Shop, Temu and modern recommendation systems do.

⸻

The architecture I’d investigate

For the engine you’ve been describing over multiple conversations:

Rust

Core:

turbovec

Storage:

sqlite

Metadata:

products
offers
inventory
facets

Embeddings:

BGE
Qwen Embedding
Gemini Embedding

Retrieval:

ANN only

Optional fallback:

FTS5

not Lucene.

⸻

The really interesting idea

I would go one step further.

Forget “search query”.

Treat everything as an embedding:

query
product
user
seller
category
brand

All become nodes in a graph.

Then:

query -> product
user -> product
product -> product
brand -> product

This starts looking less like Elasticsearch and more like a recommendation engine with a search API.

That is exactly where long-tail retrieval shines and where vectors have a structural advantage over BM25. The newest ecommerce retrieval research is moving in that direction: semantic retrieval, two-tower models, synthetic query generation, and embedding-based recall systems specifically to improve long-tail performance.  ￼

For the product you’ve been describing (embedded, local, AI-native, recommendation-oriented), I’d seriously consider:

SQLite
+
TurboVec
+
Two-tower embeddings
+
Graph relationships

and make keyword search a compatibility layer rather than the primary retrieval mechanism. That is a much more differentiated product than “another Lucene clone.”
