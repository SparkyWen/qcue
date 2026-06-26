// QCue — the cost ledger must actually be WRITTEN after a provider turn (the audit found it was
// structurally always 0: usage dropped through the turn loop, accrue never called). `accrue_turn_cost`
// is the single chokepoint RouterWikiLlm calls after run_turn; these pin its pricing + accumulation.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use app_server::dispatch::accrue_turn_cost;
use protocol::CanonicalUsage;
use sqlx::PgPool;
use store::cost_repo::CostRepo;
use uuid::Uuid;

#[sqlx::test(migrations = "../migrations")]
async fn accrue_turn_cost_grows_the_tenant_ledger(pool: PgPool) {
    let tenant = Uuid::now_v7();
    let usage = CanonicalUsage { input: 1_000_000, output: 0, ..Default::default() };

    accrue_turn_cost(&pool, tenant, "deepseek", "deepseek-chat", &usage).await;
    let repo = CostRepo::new(pool.clone());
    let (tenant_micros, _user) = repo.read_today(tenant, Uuid::nil()).await.unwrap();
    assert_eq!(tenant_micros, 280_000, "1M deepseek-chat input tokens = $0.28 = 280_000 micros");

    // a second turn accumulates (UPDATE-first path), not overwrites.
    accrue_turn_cost(&pool, tenant, "deepseek", "deepseek-chat", &usage).await;
    let (tenant_micros2, _) = repo.read_today(tenant, Uuid::nil()).await.unwrap();
    assert_eq!(tenant_micros2, 560_000);
}

#[sqlx::test(migrations = "../migrations")]
async fn keyless_stub_turn_writes_nothing(pool: PgPool) {
    let tenant = Uuid::now_v7();
    // zero usage (QCUE_STUB_LLM / empty turn) must NOT create a ledger row.
    accrue_turn_cost(&pool, tenant, "stub", "stub", &CanonicalUsage::default()).await;
    let (tenant_micros, _) = CostRepo::new(pool.clone()).read_today(tenant, Uuid::nil()).await.unwrap();
    assert_eq!(tenant_micros, 0);
}
