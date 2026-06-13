VECTORSEARCH

AI-Native Embedded Ecommerce Search Engine

Version: 0.1

Status: Draft

Author: Gleicon Moraes

⸻

1. Vision

Build a self-contained ecommerce search engine optimized for:

* Long-tail search queries
* Semantic retrieval
* Product discovery
* Recommendations
* Personalization
* Single server deployment
* Embedded usage
* Local execution
* Low memory footprint
* No Elasticsearch dependency
* No external vector database dependency

The goal is not to replace Lucene.

The goal is to build what ecommerce search becomes after embeddings.

A successful deployment should run:

* Embedded in an application
* Single binary
* Single server
* Laptop
* VPS
* Cloud VM
* Edge node

Target scale:

Metric	Goal
Products	100M+
Search latency	<50ms P95
Memory	<16GB for 100M products
Binary size	<100MB
Indexing	Online
Availability	Single node first

⸻

2. Product Philosophy

Traditional ecommerce search:

Query → BM25 → Ranking

VectorSearch:

Intent → Retrieval → Ranking

Search becomes a recommendation problem.

The engine should understand:

* intent
* similarity
* substitutes
* complements
* attributes
* brands
* user behavior

instead of matching words.

⸻

3. Core Principles

Principle 1

Vectors are primary.

Keywords are fallback.

⸻

Principle 2

Everything becomes an embedding.

Entities:

* Query
* Product
* User
* Seller
* Brand
* Category

share the same retrieval space.

⸻

Principle 3

Online updates only.

No offline PQ training.

No codebook rebuilding.

No FAISS rebuild jobs.

⸻

Principle 4

Single file storage.

No distributed system required.

Distributed deployment becomes optional.

⸻

4. High-Level Architecture

                ┌─────────────┐
                │ User Query  │
                └──────┬──────┘
                       │
                Query Encoder
                       │
                       ▼
             ┌─────────────────┐
             │ Vector Retrieval │
             └────────┬────────┘
                      │
               Top-K Candidates
                      │
                      ▼
             ┌─────────────────┐
             │ Re-ranking Layer │
             └────────┬────────┘
                      │
               Final Results
                      │
                      ▼
                 API Output

⸻

5. Technology Choices

Language

Rust

Reasons:

* SIMD
* predictable memory
* embeddable
* portable
* WASM capable
* low footprint

⸻

Storage

SQLite

Stores:

* products
* metadata
* inventory
* facets
* categories
* brands
* clicks
* purchases

Advantages:

* mature
* embeddable
* transactional
* zero operational burden

⸻

Vector Engine

TurboVec

Purpose:

* online vector indexing
* vector compression
* ANN retrieval

TurboQuant allows online quantization without training, which is attractive for ecommerce catalogs that change continuously. (Google Research￼)

⸻

Embedding Models

Initial:

* BGE Small
* Qwen Embedding

Future:

* custom ecommerce encoder

⸻

6. Product Representation

Products should not be embedded from title only.

Bad:

{
  "title": "Nike Air Max"
}

Good:

{
  "title": "Nike Air Max",
  "brand": "Nike",
  "category": "Running Shoes",
  "gender": "Male",
  "material": "Mesh",
  "description": "...",
  "attributes": {
    "color": "White",
    "size": "42"
  }
}

Structured information must influence embeddings.

⸻

7. Retrieval Model

Phase 1

Single-vector retrieval.

Query Embedding
      ↓
TurboVec
      ↓
Top-K Products

⸻

Phase 2

Two-tower retrieval.

Query Tower
      ↓
Embedding
Product Tower
      ↓
Embedding

Two-tower architectures have become a dominant pattern for semantic ecommerce retrieval and have shown improvements for long-tail queries in production systems. (arXiv￼)

⸻

8. Recommendation Model

Relationships become vectors.

Store:

product → product
user → product
seller → product
brand → product

Use:

* clicks
* purchases
* add-to-cart
* wishlist

to learn proximity.

⸻

9. Long Tail Query Strategy

Example:

gift for a guitarist under $200

Traditional search:

Few matches.

Semantic search:

Many matches.

Additional techniques:

Query Expansion

Generate:

guitar pedal
headphones
audio interface
guitar accessories

using offline models.

⸻

Query Rewriting

Inspired by Taobao and Alibaba systems.

Convert:

comfortable shoe for nurses

into:

hospital work shoes
walking shoes
orthopedic shoes

LLM-based query rewriting has shown measurable gains for long-tail ecommerce search. (arXiv￼)

⸻

10. Ranking

Ranking score:

Final Score =
Semantic Similarity
+
Popularity
+
Conversion Rate
+
Availability
+
Margin
+
Business Rules

Configurable.

⸻

11. Accuracy Measurement

Search engines fail when they optimize latency only.

Accuracy must be measured continuously.

⸻

Offline Metrics

Recall@K

Did we retrieve the purchased item?

⸻

NDCG@K

Ranking quality

⸻

MRR

Position of first correct result

⸻

Precision@K

Relevant products in top K

⸻

12. Online Metrics

Primary:

Conversion Rate

Most important metric.

Best Buy reported conversion improvements after introducing embedding-based retrieval for long-tail queries. (arXiv￼)

⸻

Secondary:

* CTR
* Add to Cart
* Revenue per Search
* Search Exit Rate
* Null Search Rate

⸻

13. Evaluation Dataset

Maintain three datasets.

Head Queries

Popular searches.

⸻

Tail Queries

Rare searches.

⸻

Human Judged

Curated relevance set.

Each query should have:

{
  "query": "shoe for standing all day",
  "relevant": [
    "sku123",
    "sku456"
  ]
}

Human evaluation should continuously validate retrieval quality. (arXiv￼)

⸻

14. Ingestion Pipeline

CSV
JSON
Database
API

↓

Normalize

↓

Generate Embedding

↓

TurboVec

↓

SQLite

⸻

Incremental updates only.

No rebuilds.

⸻

15. Public API

trait SearchEngine {
    fn index(product);
    fn delete(id);
    fn search(query);
    fn similar(product_id);
    fn recommend(user_id);
}

⸻

16. Single File Deployment

Desired deployment:

vectorsearch
catalog.db

Only.

No:

* Elasticsearch
* Kafka
* Redis
* ZooKeeper
* Kubernetes

required.

⸻

17. Algolia Killer Strategy

Algolia optimizes:

* speed
* developer experience

VectorSearch must optimize:

* intent
* discovery
* recommendation
* long-tail understanding

Differentiators:

Algolia	VectorSearch
Keyword-first	Vector-first
Search	Search + Recommendation
SaaS	Embedded
Per-record pricing	Self-hosted
Lexical relevance	Semantic relevance
External service	Local binary

⸻

18. Future Roadmap

Phase 1

* SQLite
* TurboVec
* Single vector retrieval
* REST API

Phase 2

* Two-tower training
* User embeddings
* Product relationships

Phase 3

* Personalization
* Session memory
* Real-time learning

Phase 4

* Multi-tenant SaaS
* WASM deployment
* Mobile deployment

⸻

19. Success Criteria

A successful MVP should:

* Index 10M products
* Fit inside 8GB RAM
* Return results under 50ms
* Beat BM25 on tail-query recall
* Improve conversion by >3%
* Run as a single binary on a single server

The product is not an Elasticsearch replacement.

It is a semantic retrieval and recommendation engine designed specifically for modern ecommerce catalogs where long-tail discovery matters more than lexical matching.

This spec deliberately biases toward the direction that companies like Best Buy, JD.com, Taobao and others have been moving for long-tail retrieval: semantic retrieval, two-tower models, query rewriting and embedding-driven recall rather than ever-more-complex BM25 tuning.  ￼
