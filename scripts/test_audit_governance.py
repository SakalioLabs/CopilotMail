"""Regression contracts for the source-level audit guard, never a DB audit proof."""
from pathlib import Path
import unittest

from check_audit_governance import transaction_coverage_errors
from rust_source import functions, masked, production_source


class AuditSourceTests(unittest.TestCase):
    def errors(self, source, name="use_case.rs"):
        return transaction_coverage_errors(Path(name), source)

    def test_map_error_multiline_and_plain_commits_are_all_checked(self):
        for commit in ("transaction.commit().await?;", "transaction.commit().await.map_err(map)?;",
                       "transaction\n .commit()\n .await\n .map_err(map)?;"):
            with self.subTest(commit=commit):
                self.assertEqual(len(self.errors(f"pub async fn write() {{ {commit} }}")), 1)

    def test_private_function_cannot_borrow_a_previous_functions_audit(self):
        code = """
        pub async fn first() {
            transaction.append_audit_event(&audit).await?;
            transaction.enqueue_event(&event).await?;
            transaction.commit().await?;
        }
        async fn second() { transaction.commit().await.map_err(map)?; }
        """
        self.assertEqual(len(self.errors(code)), 1)

    def test_renaming_transaction_cannot_bypass_the_guard(self):
        self.assertEqual(len(self.errors("async fn write() { tx.commit().await?; }")), 1)

    def test_evidence_on_a_different_transaction_does_not_cover_commit(self):
        code = """async fn write() {
            first.append_audit_event(a); first.enqueue_event(e);
            second.commit().await?;
        }"""
        self.assertEqual(len(self.errors(code)), 1)

    def test_helper_with_a_different_transaction_cannot_cover_the_caller(self):
        code = """async fn write() { record(&mut other).await?; tx.commit().await?; }
        async fn record(inner: &mut Tx) { inner.append_audit_event(a); inner.enqueue_event(e); }
        """
        self.assertEqual(len(self.errors(code)), 1)
        self.assertEqual(self.errors(code.replace("&mut other", "&mut tx")), [])

    def test_delegated_evidence_is_checked_through_its_actual_helper_body(self):
        prefix = """
        async fn write() { append_result(&mut transaction).await?; transaction.commit().await?; }
        async fn append_result(tx: &mut Tx) { append_pair(tx).await?; }
        async fn append_pair(tx: &mut Tx) {
            tx.append_audit_event(&audit).await?;
            tx.enqueue_event(&event).await?;
        }
        """
        self.assertEqual(self.errors(prefix), [])
        self.assertEqual(len(self.errors(prefix.replace("tx.enqueue_event(&event).await?;", ""))), 1)

    def test_unrelated_helper_and_literal_text_do_not_satisfy_evidence(self):
        code = '''
        async fn write() {
            let sample = r#"fn fake() { transaction.append_audit_event(a); transaction.enqueue_event(e); }"#;
            transaction.commit().await?;
        }
        async fn unused() { transaction.append_audit_event(a); transaction.enqueue_event(e); }
        '''
        self.assertEqual(len(self.errors(code)), 1)

    def test_comments_and_literals_cannot_fabricate_a_commit(self):
        source = '''
        // fn fake() { transaction.commit().await?; }
        /* outer /* nested */ transaction.commit().await?; */
        async fn good() { let text = "transaction.commit().await?;"; let brace = '}'; }
        '''
        self.assertEqual(self.errors(source), [])

    def test_marker_must_be_a_comment_not_a_string(self):
        body = 'let text = "audit-coverage: no-op-idempotent-replay"; transaction.commit().await?;'
        self.assertEqual(len(self.errors(f"async fn f() {{ {body} }}")), 1)
        self.assertEqual(self.errors("async fn f() {\n // audit-coverage: no-op-idempotent-replay\n transaction.commit().await?; }"), [])

    def test_cfg_test_items_do_not_hide_later_production_functions(self):
        source = """
        #[cfg(test)] mod tests { async fn mock() { transaction.commit().await?; } }
        #[cfg(test)] fn bytes() -> [u8; 32] { [0; 32] }
        async fn production() { transaction.commit().await?; }
        """
        self.assertEqual(len(self.errors(source)), 1)
        self.assertEqual([item.name for item in functions(masked(production_source(source)))], ["production"])

    def test_operational_source_requires_snapshot_and_outbox(self):
        source = """async fn reconcile() {
            transaction.append_mail_delivery_snapshot(&write).await?;
            transaction.enqueue_event(&event).await?;
            // audit-coverage: operational-source-evidence
            transaction.commit().await.map_err(map)?;
        }"""
        self.assertEqual(self.errors(source), [])
        self.assertEqual(len(self.errors(source.replace("transaction.enqueue_event(&event).await?;", ""))), 1)

    def test_lease_exception_does_not_cover_business_mutations(self):
        source = """async fn execute() {
            let mut transaction = self.store.begin().await?;
            transaction.acquire_mail_submission_lease(id).await?;
            // audit-coverage: operational-lease
            transaction.commit().await?;
        }"""
        self.assertEqual(self.errors(source, "mail_submission.rs"), [])
        self.assertEqual(len(self.errors(source)), 1)
        self.assertEqual(len(self.errors(source.replace("// audit-coverage", "transaction.insert_principal(p);\n // audit-coverage"), "mail_submission.rs")), 1)

    def test_nested_function_cannot_inherit_the_parent_evidence(self):
        source = """async fn parent() {
            transaction.append_audit_event(a); transaction.enqueue_event(e);
            async fn child() { transaction.commit().await?; }
        }"""
        self.assertEqual(len(self.errors(source)), 1)

    def test_unterminated_source_fails_instead_of_skipping_the_file(self):
        for source in ('fn f() { let s = "unfinished;', '/* unfinished', 'fn f() { let s = r#"unfinished;'):
            with self.assertRaises(ValueError): masked(source)


if __name__ == "__main__":
    unittest.main()
