export const AIRCRAFT_ISSUES = [
  ["fuel_leak_from_drain_cap", "Подтекание топлива из дренажных колпачков"],
  ["oil_stain_near_gearbox", "Масляные пятна в районе редуктора"],
  ["hydraulic_leak_on_strut", "Следы гидравлики на штоках амортизаторов шасси"],
  ["fairing_chip_or_scratch", "Сколы и царапины на обтекателях/антеннах/фонарях"],
  ["paint_peeling_at_rivets", "Отслоение краски в зонах клёпки"],
  ["missing_pitot_cover", "Отсутствие заглушек на приёмниках давления"],
  ["uneven_tread_wear", "Неравномерный износ протектора"],
  ["tire_cut_to_cord", "Порезы до корда"],
  ["low_tire_pressure", "Низкое давление в шинах"],
  ["indication_fault", "Сбои индикации (лампа, предохранитель)"],
  ["loose_connector", "Ослабленный разъём"],
  ["seatbelt_adjustment", "Регулировка привязных ремней"],
  ["burned_out_signal_lamp", "Перегоревшая светосигнальная лампа"],
  ["thrust_or_parameter_drop", "Падение тяги/оборотов/температуры газов"],
  ["excessive_vibration", "Повышенная вибрация (дисбаланс)"],
  ["metal_debris_in_oil_filter", "Стружка в маслофильтре"],
  ["radar_failure_or_false_reading", "Отказ/ложные показания РЛС"],
  ["comms_loss_or_distortion", "Потеря связи / искажение сигнала"],
  ["ins_gyro_drift", "Уход гироплатформы ИНС"],
  ["other", "Другое"],
];

export const ENGINEER_TYPES = [
  ["integrity_inspector", "Инспектор целостности и герметичности"],
  ["crew_remarks_handler", "Обработка замечаний экипажа"],
  ["fueling_crew", "Заправка (топливо/масло/кислород)"],
  ["engine_technician", "Техник по двигателю"],
  ["avionics_engineer", "Инженер по авионике"],
  ["aviation_technician", "Авиатехник (диагностика/ремонт)"],
];

const ENGINEER_TYPE_LABELS = new Map(ENGINEER_TYPES);

const ISSUE_ENGINEER = {
  fuel_leak_from_drain_cap: "integrity_inspector",
  oil_stain_near_gearbox: "integrity_inspector",
  hydraulic_leak_on_strut: "integrity_inspector",
  fairing_chip_or_scratch: "aviation_technician",
  paint_peeling_at_rivets: "aviation_technician",
  missing_pitot_cover: "fueling_crew",
  uneven_tread_wear: "fueling_crew",
  tire_cut_to_cord: "fueling_crew",
  low_tire_pressure: "fueling_crew",
  indication_fault: "crew_remarks_handler",
  loose_connector: "crew_remarks_handler",
  seatbelt_adjustment: "crew_remarks_handler",
  burned_out_signal_lamp: "crew_remarks_handler",
  thrust_or_parameter_drop: "engine_technician",
  excessive_vibration: "engine_technician",
  metal_debris_in_oil_filter: "engine_technician",
  radar_failure_or_false_reading: "avionics_engineer",
  comms_loss_or_distortion: "avionics_engineer",
  ins_gyro_drift: "avionics_engineer",
};

export const ISSUE_REQUIRED_VEHICLE = {
  fuel_leak_from_drain_cap: "FuelTruck",
  oil_stain_near_gearbox: "OilCart",
  fairing_chip_or_scratch: "MaintenanceLift",
  paint_peeling_at_rivets: "MaintenanceLift",
  missing_pitot_cover: "MaintenanceLift",
  indication_fault: "AvionicsTestSet",
  loose_connector: "AvionicsTestSet",
  thrust_or_parameter_drop: "BorescopeCart",
  excessive_vibration: "BorescopeCart",
  metal_debris_in_oil_filter: "BorescopeCart",
  radar_failure_or_false_reading: "AvionicsTestSet",
  comms_loss_or_distortion: "AvionicsTestSet",
  ins_gyro_drift: "AvionicsTestSet",
};

const VEHICLE_LABELS = {
  FuelTruck: "Топливозаправщик",
  OilCart: "Маслораздаточная тележка",
  MaintenanceLift: "Аэродромный подъёмник",
  BorescopeCart: "Бороскоп-тележка",
  AvionicsTestSet: "КПА авионики",
  Aircraft: "Воздушное судно",
  fuel_truck: "Топливозаправщик",
  oil_cart: "Маслораздаточная тележка",
  maintenance_lift: "Аэродромный подъёмник",
  borescope_cart: "Бороскоп-тележка",
  avionics_test_set: "КПА авионики",
};

export function issueGroups() {
  return [
    ["Герметичность систем", ["fuel_leak_from_drain_cap", "oil_stain_near_gearbox", "hydraulic_leak_on_strut"]],
    ["Внешние повреждения", ["fairing_chip_or_scratch", "paint_peeling_at_rivets"]],
    ["Шасси и пневматика", ["missing_pitot_cover", "uneven_tread_wear", "tire_cut_to_cord", "low_tire_pressure"]],
    ["Замечания экипажа", ["indication_fault", "loose_connector", "seatbelt_adjustment", "burned_out_signal_lamp"]],
    ["Двигатель", ["thrust_or_parameter_drop", "excessive_vibration", "metal_debris_in_oil_filter"]],
    ["Авионика", ["radar_failure_or_false_reading", "comms_loss_or_distortion", "ins_gyro_drift"]],
    ["Прочее", ["other"]],
  ];
}

export function issueLabel(value) {
  const found = AIRCRAFT_ISSUES.find(([code]) => code === value);
  return found ? found[1] : value;
}

export function engineerTypeForIssue(value) {
  const type = ISSUE_ENGINEER[value];
  if (!type) return "сервер этот тип пока не назначает";
  return ENGINEER_TYPE_LABELS.get(type) || type;
}

export function vehicleLabel(type) {
  return VEHICLE_LABELS[type] || String(type);
}

export function engineerTypeLabel(value) {
  return ENGINEER_TYPE_LABELS.get(value) || "";
}
