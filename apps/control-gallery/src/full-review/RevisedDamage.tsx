import revisedDamage from "../../../../assets/ui-review/open-play/approved-screens/implementation/shared-controls-batch-13/two-attackers/walkthrough.jpg?inline";

export default function RevisedDamage() {
  return (
    <details className="sw-revision">
      <summary>View the approved two-attacker revision</summary>
      <p>
        The live assignment uses the later layout. Compare shows the original
        screen.
      </p>
      <img
        src={revisedDamage}
        loading="lazy"
        alt="Two attackers assigning damage, revisiting the first split, and confirming both assignments"
      />
    </details>
  );
}
