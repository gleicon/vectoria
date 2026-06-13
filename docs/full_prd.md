VECTORSEARCH

AI-Native Embedded Ecommerce Search Engine

Version: 1.0
Status: Product Requirements Document (PRD)
Audience: Engineering, Product, AI/ML, Search Infrastructure

⸻

Executive Summary

VectorSearch is an AI-native ecommerce retrieval engine designed for semantic search, discovery, recommendation, and long-tail product retrieval.

Unlike traditional search engines that evolved from document retrieval systems, VectorSearch treats ecommerce search as an intent understanding problem.

The system is designed to:

* Run as a single binary
* Run on a single server
* Be embeddable inside applications
* Require minimal operational overhead
* Support tens or hundreds of millions of products
* Provide recommendation and search from the same retrieval layer
* Prioritize long-tail discovery over lexical matching

The product aims to become a self-hosted alternative to Algolia while providing superior semantic retrieval and recommendation capabilities.

⸻

Product Vision

Current ecommerce search engines are descendants of document search engines.

They were designed around:

* Keywords
* Documents
* Inverted indexes

Modern ecommerce users increasingly search using intent.

Examples:

gift for my wife who likes yoga
comfortable shoe for standing all day
camera for bird photography beginner
desk setup for remote work

These queries are recommendation problems rather than keyword matching problems.

VectorSearch is built around this assumption.

⸻

Design Principles

Principle 1

Vectors are primary.

Keywords are secondary.

⸻

Principle 2

Everything becomes an embedding.

Not only:

* products

but also:

* users
* sellers
* categories
* brands
* queries

⸻

Principle 3

Online indexing only.

The system must support continuous updates without rebuilding indexes.

⸻

Principle 4

Single file deployment.

A production deployment should consist of:

vectorsearch
catalog.db

Nothing else.

⸻

Principle 5

Search and recommendation share infrastructure.

Recommendation is not a separate subsystem.

⸻

Product Goals

Functional Goals

* Semantic search
* Product similarity
* Recommendation
* Personalization
* Explainability
* Online indexing
* Hybrid retrieval
* Multi-language support

⸻

Non Functional Goals

Metric	Target
Products	100M+
Search Latency	<50ms P95
Indexing	Online
Availability	Single Node
Binary Size	<100MB
Deployment Time	<5 minutes

⸻

System Architecture

                    User Query
                         │
                         ▼
                Query Embedding
                         │
                         ▼
                 Vector Retrieval
                         │
                         ▼
                  Candidate Set
                         │
                         ▼
                    Re-ranking
                         │
                         ▼
                 Final Results

⸻

Core Components

Storage Layer

SQLite

Stores:

* products
* metadata
* inventory
* categories
* brands
* users
* events

Reasons:

* Mature
* Reliable
* Portable
* Embedded
* Transactional

⸻

Vector Layer

TurboVec

Responsibilities:

* ANN retrieval
* compression
* insertion
* deletion

TurboVec should be treated as a vector storage engine.

Business logic remains outside.

⸻

Embedding Layer

Initial models:

* BGE Small
* E5
* Qwen Embeddings

Future:

Custom ecommerce encoder.

⸻

Retrieval Architecture

Product Embeddings

Products should be embedded using structured information.

Bad:

{
  "title": "Nike Air Max"
}

Good:

{
  "title":"Nike Air Max",
  "brand":"Nike",
  "category":"Running Shoes",
  "description":"...",
  "attributes":{
    "color":"white",
    "gender":"male"
  }
}

⸻

Query Embeddings

Flow:

Query
 ↓
Embedding
 ↓
TurboVec
 ↓
Candidates

⸻

User Embeddings

Built from:

* Clicks
* Purchases
* Views
* Wishlists

Used later for personalization.

⸻

Recommendation Engine

Relationships are represented as vectors.

Examples:

User → Product
Product → Product
Brand → Product
Seller → Product

The same retrieval infrastructure powers:

* Search
* Similar products
* Recommendations

⸻

Search Intelligence Layer

Every event is stored.

Events:

* Query
* View
* Click
* Add To Cart
* Wishlist
* Purchase

The engine continuously learns:

* Product similarity
* User affinity
* Query reformulation
* Brand affinity

Over time the intelligence layer becomes more valuable than the vector index itself.

⸻

Query Understanding

Query Expansion

Example:

gift for guitarist

Expanded into:

guitar pedal
headphones
audio interface
guitar accessories

⸻

Query Rewriting

Example:

comfortable shoe for nurses

Rewritten into:

walking shoe
orthopedic shoe
hospital shoe
standing comfort shoe

⸻

Intent Classification

Classify queries as:

* Search
* Recommendation
* Discovery
* Similarity

Example:

show me something like this

Similarity.

⸻

Ranking

Final ranking:

Score =
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

Weights are configurable.

⸻

Explainability

Every result should be explainable.

Example:

{
  "query":"shoe for nurses",
  "reasons":[
    "semantic_similarity",
    "high_conversion",
    "category_affinity"
  ]
}

Explainability is a first-class feature.

⸻

Evaluation Framework

This is the most important section of the product.

A fast search engine with poor relevance is a failure.

⸻

Ground Truth Datasets

Head Queries

Examples:

iphone
nike
airpods

Purpose:

Regression testing.

⸻

Long Tail Queries

Examples:

gift for astronomy enthusiast
shoe for nurses
camera for bird photography

Purpose:

Primary benchmark.

⸻

Human Judged Dataset

Example:

{
  "query":"comfortable office chair",
  "relevant":[
    "sku1",
    "sku2",
    "sku3"
  ]
}

Built manually.

⸻

Behavioral Dataset

Generated from:

* Clicks
* Purchases
* Cart additions
* Wishlists

Used continuously.

⸻

Offline Metrics

Recall@K

Measures retrieval quality.

Question:

Did we retrieve the relevant item?

⸻

NDCG@K

Measures ranking quality.

⸻

MRR

Measures first relevant result position.

⸻

Coverage

Queries With Results
/
Total Queries

⸻

Long Tail Recall

Custom KPI.

Measures retrieval quality for:

queries with less than 100 historical searches

This is a company-level metric.

⸻

Online Metrics

Search Conversion Rate

Primary metric.

⸻

Revenue Per Search

Revenue
/
Searches

⸻

Search Exit Rate

Must decrease over time.

⸻

Zero Result Rate

Must decrease over time.

⸻

Discovery Rate

Measures purchases of products never clicked before.

This captures discovery quality.

⸻

Human Evaluation

Quarterly review.

Sample:

1000 random queries.

Judges score:

Rating	Score
Excellent	5
Good	4
Acceptable	3
Bad	2
Wrong	1

Track trends over time.

⸻

Storage Design

Products

Stores:

* Product data
* Metadata
* Attributes

⸻

Vectors

Stores:

* Entity references
* Embedding references

Raw vectors remain inside TurboVec.

⸻

Events

Stores:

* Views
* Clicks
* Purchases
* Cart Events
* Wishlist Events

⸻

Relationships

Stores:

* Product relationships
* Brand relationships
* User relationships

⸻

Public API

Search

POST /search

⸻

Similar Products

GET /products/{id}/similar

⸻

Recommendations

GET /users/{id}/recommendations

⸻

Explain

GET /search/explain

⸻

Operations

Deployment

Single binary.

vectorsearch

Single database.

catalog.db

No requirement for:

* Elasticsearch
* Redis
* Kafka
* Kubernetes
* ZooKeeper

⸻

Backup

Backup SQLite.

Restore SQLite.

No cluster coordination required.

⸻

Multi Node (Future)

Not a primary concern.

Possible future approaches:

* Read replicas
* Sharded vector indexes
* S3 snapshots
* Object storage backups

The product should first be excellent as a single-node engine.

⸻

Competitive Positioning

Feature	Algolia	VectorSearch
Embedded	No	Yes
Self Hosted	Partial	Yes
Single Binary	No	Yes
Recommendation Native	No	Yes
Explainability	Limited	Yes
Long Tail Optimization	Limited	Primary Goal
Semantic Retrieval	Partial	Native

⸻

Success Criteria

The product is successful when:

* Long-tail recall exceeds BM25 baselines
* Discovery rate improves significantly
* Revenue per search improves
* Search conversion increases
* The engine runs on a single server
* Operations remain simple

⸻

References

TurboQuant

https://arxiv.org/abs/2504.19874

TurboVec

https://github.com/RyanCodrai/turbovec

Lucene Is All You Need

https://arxiv.org/abs/2308.14963

Best Buy Semantic Retrieval

https://arxiv.org/abs/2505.01946

Dense Passage Retrieval

https://arxiv.org/abs/2004.04906

Contriever

https://arxiv.org/abs/2112.09118

E5 Embeddings

https://arxiv.org/abs/2212.03533

BGE Embeddings

https://arxiv.org/abs/2309.07597

LLM Query Rewriting

https://arxiv.org/abs/2311.03758

Two Tower Recommendation Systems

https://research.google/pubs/deep-neural-networks-for-youtube-recommendations/

ScaNN

https://arxiv.org/abs/1908.10396

HNSW

https://arxiv.org/abs/1603.09320

Faiss

https://arxiv.org/abs/2401.08281

Product Quantization

https://hal.science/inria-00514462

⸻

End of Document.
