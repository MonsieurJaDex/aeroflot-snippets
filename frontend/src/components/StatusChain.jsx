const STEPS = ["замечание", "не назначен", "назначен", "принял", "в работе", "выполнено"];

export default function StatusChain({ current }) {
  const index = STEPS.indexOf(current);
  return (
    <p className="status-chain" aria-label={`Статус: ${current || "не выбран"}`}>
      {STEPS.map((step, i) => (
        <span key={step}>
          {i > 0 ? <span className="status-chain-sep"> → </span> : null}
          <span className={i === index ? "status-chain-step current" : i < index ? "status-chain-step done" : "status-chain-step"}>
            {step}
          </span>
        </span>
      ))}
    </p>
  );
}

export { STEPS };
