// Демо-сценарии для жюри: 5 расстановок на одной карте, 30 аккаунтов.
const DemoScenarios = (() => {
  const PASSWORD = "Demo1234";
  const ROSTER_KEY = "oto-demo-roster";
  const ACTIVE_KEY = "oto-demo-scenario";

  const SKILL = {
    integrity: "integrity_inspector",
    aviation: "aviation_technician",
    avionics: "avionics_engineer",
    engine: "engine_technician",
  };

  const SKILL_LABEL = {
    [SKILL.integrity]: "Инспектор целостности",
    [SKILL.aviation]: "Авиатехник",
    [SKILL.avionics]: "Инженер по авионике",
    [SKILL.engine]: "Техник по двигателю",
  };

  function person(name, email, role, engineerType, point) {
    return { name, email, password: PASSWORD, role, engineerType: engineerType || null, point: point || null };
  }

  function fleet(prefix, points) {
    const types = [
      ["FuelTruck", "Топливозаправщик"],
      ["OilCart", "Маслораздаточная"],
      ["MaintenanceLift", "Подъёмник"],
      ["BorescopeCart", "Бороскоп-тележка"],
      ["AvionicsTestSet", "КПА авионики"],
    ];
    return types.map(([vehicle_type, label], index) => ({
      id: `${prefix}-0000-4000-8000-${String(index + 1).padStart(12, "0")}`,
      vehicle_type,
      label,
      point: points[index],
    }));
  }

  const CALLS_TEMPLATE = [
    { stand: "A-01", issue: "fuel_leak_from_drain_cap", assigneeSlot: 0, note: "Спецтранспорт: топливозаправщик" },
    { stand: "A-02", issue: "fuel_leak_from_drain_cap", assigneeSlot: 1, note: "Нештатка 1: первый инспектор уже занят" },
    { stand: "B-01", issue: "fairing_chip_or_scratch", assigneeSlot: 2, note: "Спецтранспорт: подъёмник" },
    { stand: "B-02", issue: "radar_failure_or_false_reading", assigneeSlot: 3, note: "Спецтранспорт: КПА авионики" },
    { stand: "C-01", issue: "thrust_or_parameter_drop", assigneeSlot: 4, note: "Спецтранспорт: бороскоп-тележка" },
    { stand: "Техцентр", issue: "seatbelt_adjustment", assigneeSlot: null, note: "Нештатка 2: нет подходящего специалиста" },
  ];

  // Вариации стоянок по сценариям — те же исполнители по слотам команды
  const CALL_VARIANTS = [
    CALLS_TEMPLATE,
    [
      { stand: "B-01", issue: "fuel_leak_from_drain_cap", assigneeSlot: 0, note: "Спецтранспорт: топливозаправщик" },
      { stand: "B-02", issue: "fuel_leak_from_drain_cap", assigneeSlot: 1, note: "Нештатка 1: первый инспектор уже занят" },
      { stand: "A-01", issue: "fairing_chip_or_scratch", assigneeSlot: 2, note: "Спецтранспорт: подъёмник" },
      { stand: "C-01", issue: "radar_failure_or_false_reading", assigneeSlot: 3, note: "Спецтранспорт: КПА авионики" },
      { stand: "A-02", issue: "thrust_or_parameter_drop", assigneeSlot: 4, note: "Спецтранспорт: бороскоп-тележка" },
      { stand: "Техцентр", issue: "seatbelt_adjustment", assigneeSlot: null, note: "Нештатка 2: нет подходящего специалиста" },
    ],
    [
      { stand: "C-01", issue: "fuel_leak_from_drain_cap", assigneeSlot: 0, note: "Спецтранспорт: топливозаправщик" },
      { stand: "A-01", issue: "fuel_leak_from_drain_cap", assigneeSlot: 1, note: "Нештатка 1: первый инспектор уже занят" },
      { stand: "B-02", issue: "fairing_chip_or_scratch", assigneeSlot: 2, note: "Спецтранспорт: подъёмник" },
      { stand: "A-02", issue: "radar_failure_or_false_reading", assigneeSlot: 3, note: "Спецтранспорт: КПА авионики" },
      { stand: "B-01", issue: "thrust_or_parameter_drop", assigneeSlot: 4, note: "Спецтранспорт: бороскоп-тележка" },
      { stand: "Техцентр", issue: "seatbelt_adjustment", assigneeSlot: null, note: "Нештатка 2: нет подходящего специалиста" },
    ],
    [
      { stand: "A-02", issue: "fuel_leak_from_drain_cap", assigneeSlot: 0, note: "Спецтранспорт: топливозаправщик" },
      { stand: "C-01", issue: "fuel_leak_from_drain_cap", assigneeSlot: 1, note: "Нештатка 1: первый инспектор уже занят" },
      { stand: "B-01", issue: "fairing_chip_or_scratch", assigneeSlot: 2, note: "Спецтранспорт: подъёмник" },
      { stand: "B-02", issue: "radar_failure_or_false_reading", assigneeSlot: 3, note: "Спецтранспорт: КПА авионики" },
      { stand: "A-01", issue: "thrust_or_parameter_drop", assigneeSlot: 4, note: "Спецтранспорт: бороскоп-тележка" },
      { stand: "Техцентр", issue: "seatbelt_adjustment", assigneeSlot: null, note: "Нештатка 2: нет подходящего специалиста" },
    ],
    [
      { stand: "B-02", issue: "fuel_leak_from_drain_cap", assigneeSlot: 0, note: "Спецтранспорт: топливозаправщик" },
      { stand: "A-01", issue: "fuel_leak_from_drain_cap", assigneeSlot: 1, note: "Нештатка 1: первый инспектор уже занят" },
      { stand: "A-02", issue: "fairing_chip_or_scratch", assigneeSlot: 2, note: "Спецтранспорт: подъёмник" },
      { stand: "C-01", issue: "radar_failure_or_false_reading", assigneeSlot: 3, note: "Спецтранспорт: КПА авионики" },
      { stand: "B-01", issue: "thrust_or_parameter_drop", assigneeSlot: 4, note: "Спецтранспорт: бороскоп-тележка" },
      { stand: "Техцентр", issue: "seatbelt_adjustment", assigneeSlot: null, note: "Нештатка 2: нет подходящего специалиста" },
    ],
  ];

  const SCENARIOS = [
    {
      id: 1,
      title: "Северный перрон",
      blurb: "Базовая расстановка у стоянок A. Спецтранспорт ближе к центру.",
      dispatcher: person("Олег", "s1.disp@oto.demo", "Dispatcher"),
      engineers: [
        person("Герман", "s1.e1@oto.demo", "Engineer", SKILL.integrity, [12, 18]),
        person("Игорь", "s1.e2@oto.demo", "Engineer", SKILL.integrity, [14, 22]),
        person("Илья", "s1.e3@oto.demo", "Engineer", SKILL.aviation, [30, 16]),
        person("Никита", "s1.e4@oto.demo", "Engineer", SKILL.avionics, [34, 30]),
        person("Дмитрий", "s1.e5@oto.demo", "Engineer", SKILL.engine, [20, 40]),
      ],
      fleet: fleet("a1111111", [[18, 24], [22, 28], [40, 20], [36, 36], [28, 42]]),
      calls: CALL_VARIANTS[0],
    },
    {
      id: 2,
      title: "Восточный сектор",
      blurb: "Люди и техника смещены к стоянкам B.",
      dispatcher: person("Павел", "s2.disp@oto.demo", "Dispatcher"),
      engineers: [
        person("Кирилл", "s2.e1@oto.demo", "Engineer", SKILL.integrity, [44, 14]),
        person("Максим", "s2.e2@oto.demo", "Engineer", SKILL.integrity, [48, 20]),
        person("Андрей", "s2.e3@oto.demo", "Engineer", SKILL.aviation, [42, 32]),
        person("Егор", "s2.e4@oto.demo", "Engineer", SKILL.avionics, [50, 38]),
        person("Владимир", "s2.e5@oto.demo", "Engineer", SKILL.engine, [38, 44]),
      ],
      fleet: fleet("a2222222", [[46, 18], [50, 24], [36, 28], [42, 40], [48, 34]]),
      calls: CALL_VARIANTS[1],
    },
    {
      id: 3,
      title: "Южная зона",
      blurb: "Акцент на C-01 и техцентр, техника ниже по карте.",
      dispatcher: person("Роман", "s3.disp@oto.demo", "Dispatcher"),
      engineers: [
        person("Степан", "s3.e1@oto.demo", "Engineer", SKILL.integrity, [16, 46]),
        person("Фёдор", "s3.e2@oto.demo", "Engineer", SKILL.integrity, [22, 48]),
        person("Лев", "s3.e3@oto.demo", "Engineer", SKILL.aviation, [28, 50]),
        person("Ярослав", "s3.e4@oto.demo", "Engineer", SKILL.avionics, [34, 46]),
        person("Глеб", "s3.e5@oto.demo", "Engineer", SKILL.engine, [40, 48]),
      ],
      fleet: fleet("a3333333", [[18, 44], [24, 42], [30, 46], [36, 50], [42, 44]]),
      calls: CALL_VARIANTS[2],
    },
    {
      id: 4,
      title: "Западный рулёж",
      blurb: "Рассредоточение по левой половине перрона.",
      dispatcher: person("Сергей", "s4.disp@oto.demo", "Dispatcher"),
      engineers: [
        person("Борис", "s4.e1@oto.demo", "Engineer", SKILL.integrity, [10, 26]),
        person("Вадим", "s4.e2@oto.demo", "Engineer", SKILL.integrity, [12, 34]),
        person("Денис", "s4.e3@oto.demo", "Engineer", SKILL.aviation, [16, 30]),
        person("Захар", "s4.e4@oto.demo", "Engineer", SKILL.avionics, [14, 40]),
        person("Марат", "s4.e5@oto.demo", "Engineer", SKILL.engine, [18, 36]),
      ],
      fleet: fleet("a4444444", [[12, 22], [14, 28], [10, 32], [16, 38], [20, 34]]),
      calls: CALL_VARIANTS[3],
    },
    {
      id: 5,
      title: "Смешанный перрон",
      blurb: "Смешанная расстановка по всей карте — финальный прогон.",
      dispatcher: person("Тимур", "s5.disp@oto.demo", "Dispatcher"),
      engineers: [
        person("Руслан", "s5.e1@oto.demo", "Engineer", SKILL.integrity, [24, 20]),
        person("Тихон", "s5.e2@oto.demo", "Engineer", SKILL.integrity, [40, 24]),
        person("Юрий", "s5.e3@oto.demo", "Engineer", SKILL.aviation, [18, 34]),
        person("Эдуард", "s5.e4@oto.demo", "Engineer", SKILL.avionics, [46, 36]),
        person("Назар", "s5.e5@oto.demo", "Engineer", SKILL.engine, [32, 44]),
      ],
      fleet: fleet("a5555555", [[26, 26], [34, 22], [22, 38], [44, 32], [30, 40]]),
      calls: CALL_VARIANTS[4],
    },
  ];

  function allPeople() {
    const list = [];
    for (const scenario of SCENARIOS) {
      list.push(scenario.dispatcher);
      list.push(...scenario.engineers);
    }
    return list;
  }

  function getScenario(id) {
    return SCENARIOS.find((item) => item.id === Number(id)) || null;
  }

  function getActiveId() {
    return Number(sessionStorage.getItem(ACTIVE_KEY) || 0) || null;
  }

  function setActiveId(id) {
    if (id == null) sessionStorage.removeItem(ACTIVE_KEY);
    else sessionStorage.setItem(ACTIVE_KEY, String(id));
  }

  function getActiveScenario() {
    return getScenario(getActiveId());
  }

  function saveRoster(roster) {
    localStorage.setItem(ROSTER_KEY, JSON.stringify(roster));
  }

  function getRoster() {
    return JSON.parse(localStorage.getItem(ROSTER_KEY) || "{}");
  }

  function skillLabel(type) {
    return SKILL_LABEL[type] || type;
  }

  function callAssignee(scenario, call) {
    if (call.assigneeSlot == null) return "никто (ошибка назначения)";
    const engineer = scenario.engineers[call.assigneeSlot];
    return engineer ? engineer.name : "—";
  }

  function enrichedCalls(scenario) {
    return scenario.calls.map((call) => ({
      ...call,
      assignee: callAssignee(scenario, call),
    }));
  }

  async function enterAs(person, scenarioId) {
    setActiveId(scenarioId);
    const session = await AeroAuth.login(person.email, person.password);
    if (person.engineerType) {
      session.engineerType = person.engineerType;
      AeroAuth.saveSession(session);
    }
    window.location.href = person.role === "Dispatcher" ? "dispatcher.html" : "worker.html";
  }

  async function applyScenarioLayout(scenario) {
    if (!scenario) return;
    const roster = getRoster();
    const activeEmails = new Set([
      ...scenario.engineers.map((item) => item.email),
    ]);

    // Чужих инженеров «паркуем» в угол, чтобы /api/assign не брал их из другого сценария
    let park = 0;
    for (const person of allPeople()) {
      if (person.role !== "Engineer") continue;
      const id = roster[person.email];
      if (!id) continue;
      if (activeEmails.has(person.email)) continue;
      const point = [58 + (park % 4), 58 + Math.floor(park / 4)];
      park += 1;
      try {
        await AeroAuth.apiRequest("/api/simulate/update_engineer_position", {
          method: "POST",
          auth: false,
          body: { id, new_point: point },
        });
      } catch (error) {
        console.warn("[demo] park", person.name, error);
      }
    }

    for (const vehicle of scenario.fleet) {
      try {
        await AeroAuth.apiRequest("/api/simulate/update_transport_position", {
          method: "POST",
          auth: false,
          body: {
            id: vehicle.id,
            vehicle_type: vehicle.vehicle_type,
            new_point: vehicle.point,
          },
        });
      } catch (error) {
        console.warn("[demo] fleet", vehicle.label, error);
      }
    }

    for (const engineer of scenario.engineers) {
      const id = roster[engineer.email];
      if (!id || !engineer.point) continue;
      try {
        await AeroAuth.apiRequest("/api/simulate/update_engineer_position", {
          method: "POST",
          auth: false,
          body: { id, new_point: engineer.point },
        });
      } catch (error) {
        console.warn("[demo] engineer pos", engineer.name, error);
      }
    }
  }

  async function seedAll({ onProgress } = {}) {
    const roster = { ...getRoster() };
    const people = allPeople();
    let created = 0;
    let skipped = 0;

    for (const person of people) {
      onProgress?.(`Регистрация: ${person.name} (${person.email})`);
      try {
        const session = await AeroAuth.register({
          email: person.email,
          name: person.name,
          password: person.password,
          userRole: person.role,
          engineerType: person.engineerType || undefined,
        });
        roster[person.email] = session.userId;
        created += 1;
        AeroAuth.clearSession();
      } catch (error) {
        // уже существует — пробуем логин, чтобы вытащить id
        skipped += 1;
        try {
          const session = await AeroAuth.login(person.email, person.password);
          roster[person.email] = session.userId;
          AeroAuth.clearSession();
        } catch (loginError) {
          console.warn("[demo] seed fail", person.email, error, loginError);
        }
      }
    }

    saveRoster(roster);

    for (const scenario of SCENARIOS) {
      onProgress?.(`Расстановка сценария ${scenario.id}`);
      await applyScenarioLayout(scenario);
    }

    return { created, skipped, total: people.length };
  }

  function returnToScenarioSwitcher() {
    const id = getActiveId();
    AeroAuth.clearSession();
    if (id) window.location.href = `scenarios.html?open=${id}`;
    else window.location.href = "scenarios.html";
  }

  return {
    PASSWORD,
    SCENARIOS,
    SKILL_LABEL,
    allPeople,
    getScenario,
    getActiveId,
    setActiveId,
    getActiveScenario,
    getRoster,
    saveRoster,
    skillLabel,
    callAssignee,
    enrichedCalls,
    enterAs,
    applyScenarioLayout,
    seedAll,
    returnToScenarioSwitcher,
  };
})();
